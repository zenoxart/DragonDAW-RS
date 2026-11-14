//! The central command manager (Golden Hive's `RelayCommandPool`).
//!
//! Every user action is a registered command with a stable id. Menus, the navigation tree, the toolbar and
//! the wheel are **generated** from the commands' placements – never built by hand – and the palette searches
//! the same registry. A module's commands are removed in one call when it unloads; ids stay stable across
//! hot-swaps, so shortcut overrides and menu positions survive updates.

use redhive_contracts::api::has_permission;
use redhive_contracts::commands::{CommandDescriptor, CommandGroupDescriptor, CommandTreeNode};
use redhive_contracts::viewpack::ActionSpec;
use redhive_contracts::{IconRef, TextKey};
use std::collections::BTreeMap;

/// What running a command does.
#[derive(Debug, Clone)]
pub enum CommandHandler {
    /// Handled by the shell itself (sign out, palette, …).
    Shell,
    /// Opens a view.
    Navigate(String),
    /// Runs a module action (form → HTTP). `view` is set for row actions (they need the selected row).
    Action { module_id: String, view: Option<String>, action: ActionSpec },
}

#[derive(Debug, Clone)]
pub struct RegisteredCommand {
    pub descriptor: CommandDescriptor,
    pub handler: CommandHandler,
    pub owner: String,
}

#[derive(Default)]
pub struct CommandPool {
    commands: BTreeMap<String, RegisteredCommand>,
    groups: Vec<(String, CommandGroupDescriptor)>,
    /// Bumped on every change; views cache trees by it.
    pub revision: u64,
}

/// A palette hit.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub command_id: String,
    pub label: String,
    pub path: String,
    pub icon: IconRef,
    pub score: i32,
}

impl CommandPool {
    pub fn register(&mut self, owner: &str, descriptor: CommandDescriptor, handler: CommandHandler) {
        self.commands.insert(descriptor.id.clone(), RegisteredCommand { descriptor, handler, owner: owner.to_string() });
        self.revision += 1;
    }

    pub fn register_group(&mut self, owner: &str, group: CommandGroupDescriptor) {
        self.groups.retain(|(_, g)| !(g.menu == group.menu && g.path == group.path));
        self.groups.push((owner.to_string(), group));
        self.revision += 1;
    }

    /// Removes everything an owner (module) registered.
    pub fn unregister_owner(&mut self, owner: &str) -> usize {
        let before = self.commands.len();
        self.commands.retain(|_, c| c.owner != owner);
        self.groups.retain(|(o, _)| o != owner);
        self.revision += 1;
        before - self.commands.len()
    }

    pub fn get(&self, id: &str) -> Option<&RegisteredCommand> {
        self.commands.get(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &RegisteredCommand> {
        self.commands.values()
    }

    pub fn can_execute(&self, id: &str, permissions: &[String]) -> bool {
        self.commands.get(id).is_some_and(|c| has_permission(permissions, &c.descriptor.permission))
    }

    fn group_label(&self, menu: &str, path: &str) -> (TextKey, IconRef, i32) {
        match self.groups.iter().find(|(_, g)| g.menu == menu && g.path == path) {
            Some((_, g)) => (g.label.clone(), g.icon.clone(), g.order),
            None => {
                let seg = path.rsplit('/').next().unwrap_or(path);
                (TextKey::new(format!("shell:menu.{seg}")), IconRef::default(), 100)
            }
        }
    }

    /// Builds the tree of a placement: groups are created from paths, contributions of several modules are
    /// merged, permissions applied, items sorted by (section, order), separators inserted between sections
    /// and empty groups removed.
    pub fn tree(&self, placement: &str, permissions: &[String], shortcut_text: impl Fn(&str) -> Option<String>) -> Vec<CommandTreeNode> {
        #[derive(Default)]
        struct Node {
            children: BTreeMap<String, Node>,
            items: Vec<(i32, i32, String, CommandTreeNode)>,
        }
        let mut root = Node::default();
        for c in self.commands.values() {
            if !has_permission(permissions, &c.descriptor.permission) {
                continue;
            }
            for p in c.descriptor.placements.iter().filter(|p| p.menu == placement) {
                let mut node = &mut root;
                for seg in p.path.split('/').filter(|s| !s.is_empty()) {
                    node = node.children.entry(seg.to_string()).or_default();
                }
                node.items.push((
                    p.section,
                    p.order,
                    c.descriptor.id.clone(),
                    CommandTreeNode::Command {
                        id: format!("{placement}/{}/{}", p.path, c.descriptor.id),
                        command_id: c.descriptor.id.clone(),
                        label: c.descriptor.label.clone(),
                        icon: c.descriptor.icon.clone(),
                        shortcut: shortcut_text(&c.descriptor.id),
                    },
                ));
            }
        }
        fn build(pool: &CommandPool, placement: &str, path: &str, node: Node) -> Vec<CommandTreeNode> {
            let mut entries: Vec<(i32, i32, String, CommandTreeNode)> = node.items;
            for (seg, child) in node.children {
                let child_path = if path.is_empty() { seg.clone() } else { format!("{path}/{seg}") };
                let children = build(pool, placement, &child_path, child);
                if children.iter().all(|c| matches!(c, CommandTreeNode::Separator { .. })) {
                    continue;
                }
                let (label, icon, order) = pool.group_label(placement, &child_path);
                entries.push((-1, order, seg.clone(), CommandTreeNode::Group { id: format!("{placement}/{child_path}"), label, icon, children }));
            }
            entries.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
            let mut out = Vec::new();
            let mut last_section = None;
            for (section, _, _, node) in entries {
                if let Some(ls) = last_section {
                    if ls != section && section >= 0 && ls >= 0 {
                        out.push(CommandTreeNode::Separator { id: format!("{placement}/{path}/sep{section}") });
                    }
                }
                last_section = Some(section);
                out.push(node);
            }
            out
        }
        build(self, placement, "", root)
    }

    /// Palette search: fuzzy subsequence match over the translated label and the group path.
    pub fn search(&self, query: &str, permissions: &[String], translate: impl Fn(&str) -> String) -> Vec<SearchHit> {
        let q = query.trim().to_lowercase();
        let mut hits: Vec<SearchHit> = self
            .commands
            .values()
            .filter(|c| has_permission(permissions, &c.descriptor.permission))
            .filter_map(|c| {
                let label = translate(&c.descriptor.label.0);
                let path = c
                    .descriptor
                    .placements
                    .first()
                    .map(|p| p.path.split('/').filter(|s| !s.is_empty()).map(|seg| translate(&self.group_label(&p.menu, seg).0 .0)).collect::<Vec<_>>().join(" › "))
                    .unwrap_or_default();
                let score = if q.is_empty() { 1 } else { fuzzy_score(&q, &format!("{} {}", label.to_lowercase(), path.to_lowercase()))? };
                Some(SearchHit { command_id: c.descriptor.id.clone(), label, path, icon: c.descriptor.icon.clone(), score })
            })
            .collect();
        hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.label.cmp(&b.label)));
        hits.truncate(50);
        hits
    }
}

/// Subsequence match; consecutive and word-start matches score higher. `None` = no match.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    if text.contains(query) {
        return Some(1000 - text.find(query).unwrap_or(0) as i32);
    }
    let t: Vec<char> = text.chars().collect();
    let mut score = 0;
    let mut ti = 0;
    let mut prev_match = false;
    for qc in query.chars().filter(|c| !c.is_whitespace()) {
        let mut found = false;
        while ti < t.len() {
            let tc = t[ti];
            ti += 1;
            if tc == qc {
                score += if prev_match { 8 } else { 2 };
                if ti == 1 || t[ti - 2] == ' ' {
                    score += 6;
                }
                prev_match = true;
                found = true;
                break;
            }
            prev_match = false;
        }
        if !found {
            return None;
        }
    }
    Some(score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use redhive_contracts::commands::CommandPlacement;

    fn cmd(id: &str, path: &str, section: i32, order: i32, perm: &str) -> CommandDescriptor {
        CommandDescriptor {
            id: id.into(),
            label: TextKey::new(format!("x:{id}")),
            icon: IconRef::default(),
            permission: perm.into(),
            placements: vec![CommandPlacement::new("shell.mainmenu", path, section, order)],
            shortcuts: vec![],
            workflow_capable: false,
            ai_allowed: false,
            module_id: "x".into(),
        }
    }

    #[test]
    fn tree_merges_sorts_and_separates() {
        let mut p = CommandPool::default();
        p.register("shell", cmd("shell.exit", "file", 9, 0, ""), CommandHandler::Shell);
        p.register("shell", cmd("shell.save", "file", 0, 1, ""), CommandHandler::Shell);
        p.register("docs", cmd("docs.export", "file/export", 1, 0, "documents.view"), CommandHandler::Shell);
        p.register("docs", cmd("docs.secret", "admin", 0, 0, "admin.only"), CommandHandler::Shell);
        let t = p.tree("shell.mainmenu", &["documents.view".into()], |_| None);
        assert_eq!(t.len(), 1, "admin group without visible items is removed");
        let CommandTreeNode::Group { children, .. } = &t[0] else { panic!() };
        let kinds: Vec<&str> = children.iter().map(|c| match c { CommandTreeNode::Group { .. } => "group", CommandTreeNode::Command { command_id, .. } => command_id, CommandTreeNode::Separator { .. } => "sep" }).collect();
        assert_eq!(kinds, vec!["group", "shell.save", "sep", "shell.exit"]);
        assert_eq!(p.unregister_owner("docs"), 2);
    }

    #[test]
    fn fuzzy() {
        assert!(fuzzy_score("np", "new invoice payment").is_some());
        assert!(fuzzy_score("zz", "new invoice").is_none());
        assert!(fuzzy_score("inv", "new invoice").unwrap() > fuzzy_score("nie", "new invoice").unwrap_or(0));
    }
}
