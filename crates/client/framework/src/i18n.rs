//! Texts by key (`scope:key`) in every supported language. Lookup falls back
//! `de-AT → de → en → key`; placeholders `{name}` are filled from arguments; the language can be switched
//! live; `qps` is a pseudo-locale that makes untranslated or truncated texts visible.

use redhive_contracts::TextKey;
use redhive_monads::Error;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub const LANGUAGES: &[(&str, &str)] = &[("en", "English"), ("de", "Deutsch"), ("qps", "Pseudo")];

#[derive(Default)]
pub struct LanguageManager {
    current: String,
    texts: HashMap<String, HashMap<String, String>>,
    owners: HashMap<String, String>,
}

impl LanguageManager {
    pub fn new(language: &str) -> Self {
        let mut m = LanguageManager { current: language.to_string(), ..Default::default() };
        m.register_scope("shell", "en", SHELL_EN);
        m.register_scope("shell", "de", SHELL_DE);
        m
    }

    pub fn language(&self) -> &str {
        &self.current
    }

    pub fn set_language(&mut self, language: &str) {
        self.current = language.to_string();
    }

    /// Built-in texts: lines `scope:key = text`.
    pub fn register_scope(&mut self, owner: &str, lang: &str, table: &str) {
        for line in table.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            if let Some((k, v)) = line.split_once(" = ") {
                self.texts.entry(lang.to_string()).or_default().insert(k.trim().to_string(), v.trim().replace("\\n", "\n"));
                self.owners.insert(k.trim().to_string(), owner.to_string());
            }
        }
    }

    /// Registers a module's texts (keys without prefix).
    pub fn register_module(&mut self, module_id: &str, prefix: &str, lang: &str, texts: &std::collections::BTreeMap<String, String>) {
        let table = self.texts.entry(lang.to_string()).or_default();
        for (k, v) in texts {
            let full = format!("{prefix}:{k}");
            table.insert(full.clone(), v.clone());
            self.owners.insert(full, module_id.to_string());
        }
    }

    /// Removes everything a module registered (unload / hot-swap).
    pub fn unregister_module(&mut self, module_id: &str) {
        let keys: Vec<String> = self.owners.iter().filter(|(_, o)| o.as_str() == module_id).map(|(k, _)| k.clone()).collect();
        for k in keys {
            self.owners.remove(&k);
            for t in self.texts.values_mut() {
                t.remove(&k);
            }
        }
    }

    fn lookup(&self, key: &str) -> Option<&String> {
        let lang = if self.current == "qps" { "en" } else { self.current.as_str() };
        let mut chain = vec![lang.to_string()];
        if let Some((base, _)) = lang.split_once('-') {
            chain.push(base.to_string());
        }
        chain.push("en".into());
        chain.iter().find_map(|l| self.texts.get(l).and_then(|t| t.get(key)))
    }

    /// Translates a text key; literal text (no `scope:` prefix) is returned as is.
    pub fn t(&self, key: &str) -> String {
        if !TextKey::is_key(key) {
            return key.to_string();
        }
        let text = self.lookup(key).cloned().unwrap_or_else(|| {
            // Missing: show the last part of the key, readable.
            let k = TextKey::new(key);
            let last = k.key().rsplit('.').next().unwrap_or(key);
            let mut s: String = last.chars().flat_map(|c| if c.is_uppercase() { vec![' ', c.to_ascii_lowercase()] } else { vec![c] }).collect();
            if let Some(f) = s.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            s
        });
        if self.current == "qps" {
            pseudo(&text)
        } else {
            text
        }
    }

    /// Translates and fills `{name}` placeholders.
    pub fn t_args(&self, key: &str, args: &Map<String, Value>) -> String {
        let mut s = self.t(key);
        for (k, v) in args {
            let text = match v {
                Value::String(x) => x.clone(),
                Value::Number(n) => n.as_f64().map(|f| if f.fract() == 0.0 { format!("{}", f as i64) } else { format!("{f:.2}") }).unwrap_or_default(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            s = s.replace(&format!("{{{k}}}"), &text);
        }
        s
    }

    pub fn has(&self, key: &str) -> bool {
        self.lookup(key).is_some()
    }

    /// The user-facing message of an error: its text (with metadata placeholders) and field errors.
    pub fn error(&self, e: &Error) -> String {
        let mut s = self.t_args(&e.text_key, &e.metadata);
        if !self.has(&e.text_key) {
            s = self.t("errors:unexpected");
        }
        let fields: Vec<String> = e.field_errors.iter().filter(|f| !f.field.is_empty()).map(|f| format!("{}: {}", f.field, self.t(&f.text_key))).collect();
        if !fields.is_empty() {
            s.push_str("\n• ");
            s.push_str(&fields.join("\n• "));
        }
        s
    }

    /// Keys that exist in English but not in the current language (diagnostics).
    pub fn missing_in(&self, lang: &str) -> Vec<String> {
        let en = self.texts.get("en").cloned().unwrap_or_default();
        let other = self.texts.get(lang).cloned().unwrap_or_default();
        let mut m: Vec<String> = en.keys().filter(|k| !other.contains_key(*k)).cloned().collect();
        m.sort();
        m
    }
}

fn pseudo(s: &str) -> String {
    let mapped: String = s
        .chars()
        .map(|c| match c {
            'a' => 'á',
            'e' => 'é',
            'i' => 'ï',
            'o' => 'õ',
            'u' => 'ü',
            'A' => 'Å',
            'E' => 'Ë',
            'O' => 'Ø',
            'c' => 'ç',
            'n' => 'ñ',
            other => other,
        })
        .collect();
    format!("[!! {mapped} ·{} !!]", "~".repeat(s.chars().count() / 3))
}

const SHELL_EN: &str = r#"
shell:app.title = Red Hive
shell:common.ok = OK
shell:common.cancel = Cancel
shell:common.save = Save
shell:common.close = Close
shell:common.refresh = Refresh
shell:common.search = Search…
shell:common.loading = Loading…
shell:common.empty = Nothing here yet.
shell:common.yes = Yes
shell:common.no = No
shell:common.details = Details
shell:common.copy = Copy
shell:common.copied = Copied to the clipboard.
shell:common.confirm = Please confirm
shell:common.run = Run
shell:common.retry = Retry
shell:common.all = All
shell:common.back = Back
shell:common.result = Result
shell:login.title = Sign in to Red Hive
shell:login.server = Server
shell:login.user = User name
shell:login.password = Password
shell:login.remember = Remember me
shell:login.submit = Sign in
shell:login.busy = Signing in…
shell:login.hint = Demo users: admin, maria.huber, elena.rossi, lukas.berger, sophie.wagner – password RedHive!2026
shell:menu.file = File
shell:menu.view = View
shell:menu.workflows = Workflows
shell:menu.admin = Administration
shell:menu.help = Help
shell:menu.modules = Modules
shell:cmd.palette = Command palette
shell:cmd.signOut = Sign out
shell:cmd.exit = Exit
shell:cmd.reloadModules = Reload modules
shell:cmd.applyUpdates = Apply module updates
shell:cmd.notifications = Notifications
shell:cmd.output = Output
shell:cmd.start = Start page
shell:cmd.settings = Settings
shell:cmd.shortcuts = Keyboard shortcuts
shell:cmd.cheatSheet = Shortcut cheat sheet
shell:cmd.profile = Profile
shell:cmd.moduleExplorer = Module explorer
shell:cmd.navigation = Navigation
shell:cmd.inbox = Workflow inbox
shell:cmd.workflows = Workflows
shell:cmd.designer = Workflow designer
shell:cmd.impact = Workflow impact
shell:cmd.instances = Workflow runs
shell:cmd.aiPanel = AI agent
shell:cmd.detach = Detach view
shell:cmd.attach = Attach view
shell:cmd.closeTab = Close tab
shell:cmd.nextTab = Next tab
shell:cmd.previousTab = Previous tab
shell:cmd.resetLayout = Reset window layout
shell:cmd.saveLayout = Save layout as…
shell:cmd.help = Help for this view
shell:cmd.record = Record workflow
shell:cmd.stopRecording = Stop recording
shell:cmd.about = About Red Hive
shell:status.connected = Connected
shell:status.reconnecting = Reconnecting…
shell:status.offline = Offline
shell:status.unread = {count} unread
shell:status.modules = {count} modules
shell:status.recording = ⏺ Recording
shell:start.welcome = Welcome, {name}
shell:start.subtitle = Your modules, tasks and notifications at a glance.
shell:start.modules = Your modules
shell:start.tasks = Open tasks
shell:start.notifications = Latest notifications
shell:start.noTasks = No open tasks.
shell:notifications.title = Notifications
shell:notifications.markRead = Mark all as read
shell:notifications.empty = No notifications.
shell:notifications.clear = Clear
shell:output.title = Output
shell:modules.explorer = Module explorer
shell:modules.loaded = Loaded
shell:modules.version = Version
shell:modules.package = Package
shell:modules.signature = Signature
shell:modules.verified = verified
shell:modules.development = development module
shell:modules.rollouts = Rollouts
shell:modules.advance = Advance
shell:modules.force = Force
shell:modules.hold = Hold
shell:modules.cancel = Cancel (roll back)
shell:modules.publish = Publish manifest…
shell:modules.publishHint = Paste a module manifest (JSON) to publish it.
shell:modules.published = {module} {version} was published
shell:modules.rejected = Module {module} was not loaded: {reason}
shell:modules.newer = newer version {version} is being rolled out
shell:updates.available = {module} {version} is available
shell:updates.held = The rollout of {module} is held: {count} workflow(s) would break
shell:updates.swapped = {module} was updated to {version}
shell:updates.rolledBack = {module} was rolled back to {version}
shell:updates.restart = {module} {version} needs a restart of Red Hive
shell:updates.failed = Updating {module} failed – the previous version stays active
shell:settings.title = Settings
shell:settings.language = Language
shell:settings.theme = Theme
shell:settings.themeDark = Dark
shell:settings.themeLight = Light
shell:settings.themeSystem = System
shell:settings.scale = Zoom
shell:settings.osToasts = Show Windows notifications
shell:settings.reducedMotion = Reduce motion
shell:shortcuts.title = Keyboard shortcuts
shell:shortcuts.command = Command
shell:shortcuts.gesture = Shortcut
shell:shortcuts.record = Record
shell:shortcuts.recording = Press the keys…
shell:shortcuts.reset = Reset
shell:shortcuts.resetAll = Reset all
shell:shortcuts.conflict = Conflict with {command}
shell:shortcuts.reserved = Reserved for the shell
shell:shortcuts.export = Export
shell:shortcuts.import = Import
shell:shortcuts.preset = Preset
shell:shortcuts.cheatSheet = Shortcuts available now
shell:profile.title = Profile
shell:profile.roles = Roles
shell:profile.limit = Approval limit
shell:profile.picture = Profile picture
shell:profile.choosePicture = Choose picture…
shell:profile.removePicture = Remove picture
shell:profile.picturePath = Path to a PNG or JPEG file
shell:profile.password = Change password
shell:profile.current = Current password
shell:profile.new = New password
shell:profile.repeat = Repeat new password
shell:profile.mismatch = The passwords do not match.
shell:profile.changed = Your password was changed.
shell:profile.pictureSaved = Your profile picture was saved.
shell:profile.lastLogin = Last sign-in
shell:palette.placeholder = Type a command…
shell:palette.noResults = No matching command.
shell:wheel.title = Navigation
shell:view.notFound = The view {view} is not available.
shell:view.placeholder = The module of this tab is not loaded.
shell:view.detached = This view is open in its own window.
shell:list.items = {count} items
shell:list.select = Select a row to see details.
shell:list.actions = Actions
shell:list.groupNone = (none)
shell:form.required = Required
shell:form.choose = Choose…
shell:form.lookupFailed = The list could not be loaded.
shell:report.parameters = Parameters
shell:report.copyCsv = Copy as CSV
shell:report.total = Total
shell:report.choose = Choose a report on the left.
shell:report.recent = Recent runs
shell:about.text = Red Hive – a modular, server-driven enterprise framework in Rust.
shell:help.text = F1: help · Ctrl+Shift+P: command palette · Ctrl+Tab (hold): navigation wheel · Ctrl+/: cheat sheet · Ctrl+Shift+D: detach view
shell:size.compact = Compact
shell:size.medium = Medium
shell:size.expanded = Expanded
shell:size.wide = Wide
errors:unexpected = Something went wrong. Please try again.
errors:unauthorized = Please sign in.
errors:sessionExpired = Your session has expired – please sign in again.
errors:forbidden = You do not have the permission for this ({permission}).
errors:notFound = Not found.
errors:required = Required.
errors:invalidValue = Invalid value.
errors:invalidJson = The request could not be read.
errors:duplicate = This entry already exists.
errors:reference = The entry is still in use or refers to something that does not exist.
errors:staleVersion = Someone else changed this in the meantime. Reload and try again.
errors:unavailable = A service is not available right now. Please try again shortly.
errors:serverUnavailable = The server cannot be reached.
errors:timeout = The server did not answer in time.
errors:rateLimited = Too many requests – please wait a moment.
errors:tooLarge = The request is too large.
errors:validationFailed = Please check your input.
errors:number = Please enter a number.
errors:min = The value must be at least {min}.
errors:max = The value must be at most {max}.
errors:maxLength = At most {maxLength} characters.
errors:pattern = The format is not valid.
errors:choice = Please choose one of the options.
errors:date = Please enter a date (YYYY-MM-DD).
errors:boolean = Please choose yes or no.
identity:login.invalid = User name or password is wrong.
identity:login.inactive = This account is deactivated.
identity:login.missing = Please enter user name and password.
identity:password.weak = At least 10 characters with upper and lower case letters, a digit and a symbol.
identity:password.currentWrong = The current password is wrong.
identity:password.same = The new password must be different.
identity:avatar.type = Only PNG or JPEG pictures.
identity:avatar.tooLarge = The picture is larger than {maxKb} KB.
identity:avatar.data = The picture could not be read.
identity:avatar.none = No picture.
modules:host.tooOld = This module needs Red Hive {required} or later.
modules:contracts.unsupported = The module uses unsupported contracts.
modules:dependency.missing = It needs the module {dependency}.
modules:dependency.version = It needs {dependency} {range} (found {found}).
modules:dependency.rejected = Its dependency {dependency} was not loaded.
modules:dependency.cycle = Its dependencies form a cycle.
modules:version.notNewer = The version must be newer than {latest}.
modules:version.notFound = The module version does not exist.
modules:signature.invalid = The package signature is not valid.
modules:hash.invalid = The package is damaged (hash mismatch).
modules:signingKey.changed = The server's signing key changed – packages are refused until you trust the new key.
modules:package.missing = There is no package for this module version.
delivery:package.invalid = The package is not valid.
delivery:package.immutable = A published package cannot be replaced.
updates:rollout.broken = Workflows would break – review them or force the rollout.
updates:rollout.finished = The rollout is finished.
updates:rollout.notActive = The rollout is not running.
updates:rollout.notFound = The rollout does not exist.
config:setting.notFound = The setting does not exist.
ai:disabled = The AI agent is switched off.
ai:lowConfidence = The AI agent is not sure enough – a person decides.
ai:stepNotAllowed = The AI agent may not run this step.
ai:limitRunsPerDay = The AI agent reached its daily limit.
ai:limitSteps = The AI agent reached the step limit of this run.
ai:limitTokens = The AI agent reached the token limit of this run.
ai:limitTime = The AI agent reached the time limit of this run.
ai:loopDetected = The AI agent repeated a step too often.
ai:proposalInvalid = The AI agent's proposal was not valid.
ai:provider.unavailable = The AI provider is not available.
ai:notification.enabled = The AI agent was switched on
ai:notification.disabled = The AI agent was switched off
ai:panel.title = AI agent
ai:panel.status = Status
ai:panel.enabled = On
ai:panel.disabled = Off (kill switch)
ai:panel.provider = Provider
ai:panel.runsToday = Runs today
ai:panel.switchOff = Switch the AI agent off
ai:panel.switchOn = Switch the AI agent on
ai:panel.audit = Audit trail
ai:panel.decision = Decision
ai:panel.confidence = Confidence
ai:panel.reasoning = Reasoning
workflow:inbox.title = Workflow inbox
workflow:inbox.empty = No tasks for you.
workflow:inbox.aiProposal = AI proposal
workflow:inbox.confidence = confidence {value}
workflow:inbox.confirmAi = The AI agent wants to run this step. Confirm it (it runs with your permissions) or cancel the run.
workflow:inbox.history = History
workflow:inbox.submit = Submit
workflow:inbox.runAs = runs as {name}
workflow:inbox.useProposal = Use proposal
workflow:decision.approve = Approve
workflow:decision.reject = Reject
workflow:decision.confirm = Confirm
workflow:decision.cancel = Cancel the run
workflow:decision.required = Please decide.
workflow:decision.invalid = Unknown option.
workflow:comment.required = Please give a reason.
workflow:list.title = Workflows
workflow:list.start = Start
workflow:list.startAi = Start with AI
workflow:list.edit = Edit
workflow:list.retire = Retire
workflow:list.new = New workflow
workflow:list.running = {count} running
workflow:designer.title = Workflow designer
workflow:designer.validate = Validate
workflow:designer.saveDraft = Save draft
workflow:designer.publish = Publish
workflow:designer.catalog = Building blocks
workflow:designer.issues = Issues
workflow:designer.valid = No issues.
workflow:designer.saved = Saved as version {version}.
workflow:designer.addStep = Add step
workflow:designer.json = Definition (JSON)
workflow:recorder.started = Recording – run commands, then stop to get a workflow draft.
workflow:recorder.stopped = Recorded {count} step(s) – opened in the designer.
workflow:instances.title = Workflow runs
workflow:instances.cancel = Cancel run
workflow:instances.resume = Resume
workflow:impact.title = Workflow impact
workflow:impact.empty = No module updates analysed yet.
workflow:impact.changes = Contract changes
workflow:impact.workflows = Affected workflows
workflow:impact.compatible = compatible
workflow:impact.autoMigrated = auto-migrated
workflow:impact.reviewRequired = needs review
workflow:impact.broken = broken
workflow:notification.task = New task: {step} ({workflow})
workflow:notification.approval = Approval needed: {step} ({workflow})
workflow:notification.aiApproval = The AI agent asks for confirmation: {step} ({workflow})
workflow:notification.completed = Workflow "{workflow}" completed
workflow:notification.failed = Workflow "{workflow}" failed
workflow:notification.impact = {module} {version}: {migrated} workflow(s) migrated
workflow:notification.impactAttention = {module} {version}: {review} workflow(s) need review, {broken} broken
workflow:definition.invalid = The workflow is not valid.
workflow:definition.notFound = The workflow does not exist.
workflow:definition.notActive = The workflow is not active.
workflow:definition.notRunnable = The workflow cannot run in its current state.
workflow:instance.notFound = The workflow run does not exist.
workflow:instance.finished = The run has already ended.
workflow:instance.notPaused = The run is not paused.
workflow:task.none = There is no open task.
workflow:task.notAssigned = This task is not assigned to you.
workflow:stepType.unknown = Unknown step type.
workflow:step.missing = The step does not exist.
workflow:expression.invalid = The expression is not valid.
workflow:loopDetected = The workflow ran too many steps without waiting.
workflow:ai.off = The AI agent is off for this workflow.
workflow:ai.permission = You may not hand workflows to the AI agent.
workflow:runAs.inactive = The user the workflow runs as is deactivated.
workflow:field.result = Result
"#;

const SHELL_DE: &str = r#"
shell:app.title = Red Hive
shell:common.ok = OK
shell:common.cancel = Abbrechen
shell:common.save = Speichern
shell:common.close = Schließen
shell:common.refresh = Aktualisieren
shell:common.search = Suchen…
shell:common.loading = Wird geladen…
shell:common.empty = Noch nichts vorhanden.
shell:common.yes = Ja
shell:common.no = Nein
shell:common.details = Details
shell:common.copy = Kopieren
shell:common.copied = In die Zwischenablage kopiert.
shell:common.confirm = Bitte bestätigen
shell:common.run = Ausführen
shell:common.retry = Erneut versuchen
shell:common.all = Alle
shell:common.back = Zurück
shell:common.result = Ergebnis
shell:login.title = Bei Red Hive anmelden
shell:login.server = Server
shell:login.user = Benutzername
shell:login.password = Passwort
shell:login.remember = Angemeldet bleiben
shell:login.submit = Anmelden
shell:login.busy = Anmeldung läuft…
shell:login.hint = Demo-Benutzer: admin, maria.huber, elena.rossi, lukas.berger, sophie.wagner – Passwort RedHive!2026
shell:menu.file = Datei
shell:menu.view = Ansicht
shell:menu.workflows = Workflows
shell:menu.admin = Verwaltung
shell:menu.help = Hilfe
shell:menu.modules = Module
shell:cmd.palette = Befehlspalette
shell:cmd.signOut = Abmelden
shell:cmd.exit = Beenden
shell:cmd.reloadModules = Module neu laden
shell:cmd.applyUpdates = Modul-Updates anwenden
shell:cmd.notifications = Benachrichtigungen
shell:cmd.output = Ausgabe
shell:cmd.start = Startseite
shell:cmd.settings = Einstellungen
shell:cmd.shortcuts = Tastenkombinationen
shell:cmd.cheatSheet = Tastenkürzel-Übersicht
shell:cmd.profile = Profil
shell:cmd.moduleExplorer = Modul-Explorer
shell:cmd.navigation = Navigation
shell:cmd.inbox = Workflow-Eingang
shell:cmd.workflows = Workflows
shell:cmd.designer = Workflow-Designer
shell:cmd.impact = Workflow-Auswirkungen
shell:cmd.instances = Workflow-Läufe
shell:cmd.aiPanel = KI-Agent
shell:cmd.detach = Ansicht abdocken
shell:cmd.attach = Ansicht andocken
shell:cmd.closeTab = Tab schließen
shell:cmd.nextTab = Nächster Tab
shell:cmd.previousTab = Vorheriger Tab
shell:cmd.resetLayout = Fensterlayout zurücksetzen
shell:cmd.saveLayout = Layout speichern unter…
shell:cmd.help = Hilfe zu dieser Ansicht
shell:cmd.record = Workflow aufzeichnen
shell:cmd.stopRecording = Aufzeichnung beenden
shell:cmd.about = Über Red Hive
shell:status.connected = Verbunden
shell:status.reconnecting = Verbindung wird hergestellt…
shell:status.offline = Offline
shell:status.unread = {count} ungelesen
shell:status.modules = {count} Module
shell:status.recording = ⏺ Aufzeichnung
shell:start.welcome = Willkommen, {name}
shell:start.subtitle = Ihre Module, Aufgaben und Benachrichtigungen auf einen Blick.
shell:start.modules = Ihre Module
shell:start.tasks = Offene Aufgaben
shell:start.notifications = Neueste Benachrichtigungen
shell:start.noTasks = Keine offenen Aufgaben.
shell:notifications.title = Benachrichtigungen
shell:notifications.markRead = Alle als gelesen markieren
shell:notifications.empty = Keine Benachrichtigungen.
shell:notifications.clear = Leeren
shell:output.title = Ausgabe
shell:modules.explorer = Modul-Explorer
shell:modules.loaded = Geladen
shell:modules.version = Version
shell:modules.package = Paket
shell:modules.signature = Signatur
shell:modules.verified = geprüft
shell:modules.development = Entwicklungsmodul
shell:modules.rollouts = Rollouts
shell:modules.advance = Weiter
shell:modules.force = Erzwingen
shell:modules.hold = Anhalten
shell:modules.cancel = Abbrechen (zurückrollen)
shell:modules.publish = Manifest veröffentlichen…
shell:modules.publishHint = Fügen Sie ein Modul-Manifest (JSON) ein, um es zu veröffentlichen.
shell:modules.published = {module} {version} wurde veröffentlicht
shell:modules.rejected = Modul {module} wurde nicht geladen: {reason}
shell:modules.newer = neuere Version {version} wird ausgerollt
shell:updates.available = {module} {version} ist verfügbar
shell:updates.held = Der Rollout von {module} ist angehalten: {count} Workflow(s) würden brechen
shell:updates.swapped = {module} wurde auf {version} aktualisiert
shell:updates.rolledBack = {module} wurde auf {version} zurückgesetzt
shell:updates.restart = {module} {version} erfordert einen Neustart von Red Hive
shell:updates.failed = Das Update von {module} ist fehlgeschlagen – die bisherige Version bleibt aktiv
shell:settings.title = Einstellungen
shell:settings.language = Sprache
shell:settings.theme = Design
shell:settings.themeDark = Dunkel
shell:settings.themeLight = Hell
shell:settings.themeSystem = System
shell:settings.scale = Zoom
shell:settings.osToasts = Windows-Benachrichtigungen anzeigen
shell:settings.reducedMotion = Animationen reduzieren
shell:shortcuts.title = Tastenkombinationen
shell:shortcuts.command = Befehl
shell:shortcuts.gesture = Tastenkürzel
shell:shortcuts.record = Aufnehmen
shell:shortcuts.recording = Tasten drücken…
shell:shortcuts.reset = Zurücksetzen
shell:shortcuts.resetAll = Alle zurücksetzen
shell:shortcuts.conflict = Konflikt mit {command}
shell:shortcuts.reserved = Für die Shell reserviert
shell:shortcuts.export = Exportieren
shell:shortcuts.import = Importieren
shell:shortcuts.preset = Vorlage
shell:shortcuts.cheatSheet = Jetzt verfügbare Tastenkürzel
shell:profile.title = Profil
shell:profile.roles = Rollen
shell:profile.limit = Freigabelimit
shell:profile.picture = Profilbild
shell:profile.choosePicture = Bild auswählen…
shell:profile.removePicture = Bild entfernen
shell:profile.picturePath = Pfad zu einer PNG- oder JPEG-Datei
shell:profile.password = Passwort ändern
shell:profile.current = Aktuelles Passwort
shell:profile.new = Neues Passwort
shell:profile.repeat = Neues Passwort wiederholen
shell:profile.mismatch = Die Passwörter stimmen nicht überein.
shell:profile.changed = Ihr Passwort wurde geändert.
shell:profile.pictureSaved = Ihr Profilbild wurde gespeichert.
shell:profile.lastLogin = Letzte Anmeldung
shell:palette.placeholder = Befehl eingeben…
shell:palette.noResults = Kein passender Befehl.
shell:wheel.title = Navigation
shell:view.notFound = Die Ansicht {view} ist nicht verfügbar.
shell:view.placeholder = Das Modul dieses Tabs ist nicht geladen.
shell:view.detached = Diese Ansicht ist in einem eigenen Fenster geöffnet.
shell:list.items = {count} Einträge
shell:list.select = Wählen Sie eine Zeile, um Details zu sehen.
shell:list.actions = Aktionen
shell:list.groupNone = (keine)
shell:form.required = Pflichtfeld
shell:form.choose = Auswählen…
shell:form.lookupFailed = Die Liste konnte nicht geladen werden.
shell:report.parameters = Parameter
shell:report.copyCsv = Als CSV kopieren
shell:report.total = Summe
shell:report.choose = Wählen Sie links einen Bericht.
shell:report.recent = Letzte Läufe
shell:about.text = Red Hive – ein modulares, servergesteuertes Enterprise-Framework in Rust.
shell:help.text = F1: Hilfe · Strg+Umschalt+P: Befehlspalette · Strg+Tab (halten): Navigationsrad · Strg+/: Tastenkürzel · Strg+Umschalt+D: Ansicht abdocken
shell:size.compact = Kompakt
shell:size.medium = Mittel
shell:size.expanded = Erweitert
shell:size.wide = Breit
errors:unexpected = Etwas ist schiefgelaufen. Bitte versuchen Sie es erneut.
errors:unauthorized = Bitte melden Sie sich an.
errors:sessionExpired = Ihre Sitzung ist abgelaufen – bitte melden Sie sich erneut an.
errors:forbidden = Ihnen fehlt die Berechtigung dafür ({permission}).
errors:notFound = Nicht gefunden.
errors:required = Pflichtfeld.
errors:invalidValue = Ungültiger Wert.
errors:invalidJson = Die Anfrage konnte nicht gelesen werden.
errors:duplicate = Dieser Eintrag existiert bereits.
errors:reference = Der Eintrag wird noch verwendet oder verweist auf etwas, das nicht existiert.
errors:staleVersion = Jemand anderes hat das inzwischen geändert. Neu laden und erneut versuchen.
errors:unavailable = Ein Dienst ist gerade nicht erreichbar. Bitte versuchen Sie es gleich noch einmal.
errors:serverUnavailable = Der Server ist nicht erreichbar.
errors:timeout = Der Server hat nicht rechtzeitig geantwortet.
errors:rateLimited = Zu viele Anfragen – bitte kurz warten.
errors:tooLarge = Die Anfrage ist zu groß.
errors:validationFailed = Bitte prüfen Sie Ihre Eingaben.
errors:number = Bitte eine Zahl eingeben.
errors:min = Der Wert muss mindestens {min} sein.
errors:max = Der Wert darf höchstens {max} sein.
errors:maxLength = Höchstens {maxLength} Zeichen.
errors:pattern = Das Format ist ungültig.
errors:choice = Bitte eine der Optionen wählen.
errors:date = Bitte ein Datum eingeben (JJJJ-MM-TT).
errors:boolean = Bitte Ja oder Nein wählen.
identity:login.invalid = Benutzername oder Passwort ist falsch.
identity:login.inactive = Dieses Konto ist deaktiviert.
identity:login.missing = Bitte Benutzername und Passwort eingeben.
identity:password.weak = Mindestens 10 Zeichen mit Groß- und Kleinbuchstaben, einer Ziffer und einem Sonderzeichen.
identity:password.currentWrong = Das aktuelle Passwort ist falsch.
identity:password.same = Das neue Passwort muss sich unterscheiden.
identity:avatar.type = Nur PNG- oder JPEG-Bilder.
identity:avatar.tooLarge = Das Bild ist größer als {maxKb} KB.
identity:avatar.data = Das Bild konnte nicht gelesen werden.
identity:avatar.none = Kein Bild.
modules:host.tooOld = Dieses Modul benötigt Red Hive {required} oder neuer.
modules:contracts.unsupported = Das Modul verwendet nicht unterstützte Verträge.
modules:dependency.missing = Es benötigt das Modul {dependency}.
modules:dependency.version = Es benötigt {dependency} {range} (gefunden {found}).
modules:dependency.rejected = Seine Abhängigkeit {dependency} wurde nicht geladen.
modules:dependency.cycle = Seine Abhängigkeiten bilden einen Zyklus.
modules:version.notNewer = Die Version muss neuer als {latest} sein.
modules:version.notFound = Die Modulversion existiert nicht.
modules:signature.invalid = Die Paketsignatur ist ungültig.
modules:hash.invalid = Das Paket ist beschädigt (Prüfsumme falsch).
modules:signingKey.changed = Der Signaturschlüssel des Servers hat sich geändert – Pakete werden abgelehnt, bis Sie dem neuen Schlüssel vertrauen.
modules:package.missing = Für diese Modulversion gibt es kein Paket.
delivery:package.invalid = Das Paket ist ungültig.
delivery:package.immutable = Ein veröffentlichtes Paket kann nicht ersetzt werden.
updates:rollout.broken = Workflows würden brechen – prüfen Sie sie oder erzwingen Sie den Rollout.
updates:rollout.finished = Der Rollout ist abgeschlossen.
updates:rollout.notActive = Der Rollout läuft nicht.
updates:rollout.notFound = Der Rollout existiert nicht.
config:setting.notFound = Die Einstellung existiert nicht.
ai:disabled = Der KI-Agent ist abgeschaltet.
ai:lowConfidence = Der KI-Agent ist sich nicht sicher genug – eine Person entscheidet.
ai:stepNotAllowed = Der KI-Agent darf diesen Schritt nicht ausführen.
ai:limitRunsPerDay = Der KI-Agent hat sein Tageslimit erreicht.
ai:limitSteps = Der KI-Agent hat das Schrittlimit dieses Laufs erreicht.
ai:limitTokens = Der KI-Agent hat das Token-Limit dieses Laufs erreicht.
ai:limitTime = Der KI-Agent hat das Zeitlimit dieses Laufs erreicht.
ai:loopDetected = Der KI-Agent hat einen Schritt zu oft wiederholt.
ai:proposalInvalid = Der Vorschlag des KI-Agenten war ungültig.
ai:provider.unavailable = Der KI-Anbieter ist nicht erreichbar.
ai:notification.enabled = Der KI-Agent wurde eingeschaltet
ai:notification.disabled = Der KI-Agent wurde abgeschaltet
ai:panel.title = KI-Agent
ai:panel.status = Status
ai:panel.enabled = Ein
ai:panel.disabled = Aus (Not-Aus)
ai:panel.provider = Anbieter
ai:panel.runsToday = Läufe heute
ai:panel.switchOff = KI-Agent abschalten
ai:panel.switchOn = KI-Agent einschalten
ai:panel.audit = Prüfprotokoll
ai:panel.decision = Entscheidung
ai:panel.confidence = Sicherheit
ai:panel.reasoning = Begründung
workflow:inbox.title = Workflow-Eingang
workflow:inbox.empty = Keine Aufgaben für Sie.
workflow:inbox.aiProposal = KI-Vorschlag
workflow:inbox.confidence = Sicherheit {value}
workflow:inbox.confirmAi = Der KI-Agent möchte diesen Schritt ausführen. Bestätigen Sie ihn (er läuft mit Ihren Berechtigungen) oder brechen Sie den Lauf ab.
workflow:inbox.history = Verlauf
workflow:inbox.submit = Absenden
workflow:inbox.runAs = läuft als {name}
workflow:inbox.useProposal = Vorschlag übernehmen
workflow:decision.approve = Freigeben
workflow:decision.reject = Ablehnen
workflow:decision.confirm = Bestätigen
workflow:decision.cancel = Lauf abbrechen
workflow:decision.required = Bitte entscheiden Sie.
workflow:decision.invalid = Unbekannte Option.
workflow:comment.required = Bitte geben Sie eine Begründung an.
workflow:list.title = Workflows
workflow:list.start = Starten
workflow:list.startAi = Mit KI starten
workflow:list.edit = Bearbeiten
workflow:list.retire = Stilllegen
workflow:list.new = Neuer Workflow
workflow:list.running = {count} laufend
workflow:designer.title = Workflow-Designer
workflow:designer.validate = Prüfen
workflow:designer.saveDraft = Entwurf speichern
workflow:designer.publish = Veröffentlichen
workflow:designer.catalog = Bausteine
workflow:designer.issues = Hinweise
workflow:designer.valid = Keine Hinweise.
workflow:designer.saved = Als Version {version} gespeichert.
workflow:designer.addStep = Schritt hinzufügen
workflow:designer.json = Definition (JSON)
workflow:recorder.started = Aufzeichnung läuft – führen Sie Befehle aus und beenden Sie dann die Aufzeichnung.
workflow:recorder.stopped = {count} Schritt(e) aufgezeichnet – im Designer geöffnet.
workflow:instances.title = Workflow-Läufe
workflow:instances.cancel = Lauf abbrechen
workflow:instances.resume = Fortsetzen
workflow:impact.title = Workflow-Auswirkungen
workflow:impact.empty = Noch keine Modul-Updates analysiert.
workflow:impact.changes = Vertragsänderungen
workflow:impact.workflows = Betroffene Workflows
workflow:impact.compatible = kompatibel
workflow:impact.autoMigrated = automatisch migriert
workflow:impact.reviewRequired = Prüfung nötig
workflow:impact.broken = gebrochen
workflow:notification.task = Neue Aufgabe: {step} ({workflow})
workflow:notification.approval = Freigabe nötig: {step} ({workflow})
workflow:notification.aiApproval = Der KI-Agent bittet um Bestätigung: {step} ({workflow})
workflow:notification.completed = Workflow „{workflow}“ abgeschlossen
workflow:notification.failed = Workflow „{workflow}“ fehlgeschlagen
workflow:notification.impact = {module} {version}: {migrated} Workflow(s) migriert
workflow:notification.impactAttention = {module} {version}: {review} Workflow(s) prüfen, {broken} gebrochen
workflow:definition.invalid = Der Workflow ist ungültig.
workflow:definition.notFound = Der Workflow existiert nicht.
workflow:definition.notActive = Der Workflow ist nicht aktiv.
workflow:definition.notRunnable = Der Workflow kann in seinem aktuellen Zustand nicht laufen.
workflow:instance.notFound = Der Workflow-Lauf existiert nicht.
workflow:instance.finished = Der Lauf ist bereits beendet.
workflow:instance.notPaused = Der Lauf ist nicht angehalten.
workflow:task.none = Es gibt keine offene Aufgabe.
workflow:task.notAssigned = Diese Aufgabe ist Ihnen nicht zugewiesen.
workflow:stepType.unknown = Unbekannter Schritttyp.
workflow:step.missing = Der Schritt existiert nicht.
workflow:expression.invalid = Der Ausdruck ist ungültig.
workflow:loopDetected = Der Workflow hat zu viele Schritte ohne Pause ausgeführt.
workflow:ai.off = Der KI-Agent ist für diesen Workflow aus.
workflow:ai.permission = Sie dürfen Workflows nicht an den KI-Agenten übergeben.
workflow:runAs.inactive = Der Benutzer, als der der Workflow läuft, ist deaktiviert.
workflow:field.result = Ergebnis
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_fallback_and_args() {
        let mut m = LanguageManager::new("de-AT");
        assert_eq!(m.t("shell:common.save"), "Speichern");
        let mut a = Map::new();
        a.insert("count".into(), serde_json::json!(3));
        assert_eq!(m.t_args("shell:status.unread", &a), "3 ungelesen");
        m.set_language("en");
        assert_eq!(m.t("shell:common.save"), "Save");
        assert_eq!(m.t("plain text"), "plain text");
        assert_eq!(m.t("x:some.missingKey"), "Missing key");
        m.register_module("docs", "documents", "en", &[("view.home".to_string(), "Home".to_string())].into());
        assert_eq!(m.t("documents:view.home"), "Home");
        m.unregister_module("docs");
        assert!(!m.has("documents:view.home"));
        assert!(m.missing_in("de").is_empty(), "{:?}", m.missing_in("de"));
    }
}
