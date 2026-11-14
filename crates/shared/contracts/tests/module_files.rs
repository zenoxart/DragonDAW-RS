//! Checks the shipped modules: manifests, view packs (incl. text completeness) and workflow templates,
//! and the classification of the demo update (document-management 1.1.0).

use redhive_contracts::modules::{check_manifest, diff_surfaces, ModuleManifest};
use redhive_contracts::viewpack::ViewPack;
use redhive_contracts::workflows::analysis::ensure_valid;
use redhive_contracts::workflows::impact::{analyze, ImpactClass};
use redhive_contracts::workflows::WorkflowDefinition;
use std::path::{Path, PathBuf};

fn modules_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../modules")
}

fn read<T: serde::de::DeserializeOwned>(p: &Path) -> T {
    serde_json::from_str(&std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn templates() -> Vec<WorkflowDefinition> {
    let mut out = Vec::new();
    for m in std::fs::read_dir(modules_dir()).unwrap().flatten() {
        if let Ok(files) = std::fs::read_dir(m.path().join("workflows")) {
            for f in files.flatten() {
                out.push(read::<WorkflowDefinition>(&f.path()));
            }
        }
    }
    out
}

#[test]
fn manifests_view_packs_and_templates_are_valid() {
    let mut count = 0;
    for m in std::fs::read_dir(modules_dir()).unwrap().flatten() {
        let manifest: ModuleManifest = read(&m.path().join("manifest.json"));
        check_manifest(&manifest, "1.0.0").unwrap();
        let pack: ViewPack = read(&m.path().join("client").join(&manifest.client.view_pack));
        assert_eq!(pack.module_id, manifest.module_id);
        assert_eq!(pack.version, manifest.version);
        let missing = pack.missing_texts(&manifest.prefix);
        assert!(missing.is_empty(), "{}: missing texts {missing:?}", manifest.module_id);
        for lang in &manifest.languages {
            assert!(pack.texts.contains_key(lang), "{}: no texts for {lang}", manifest.module_id);
        }
        count += 1;
    }
    assert_eq!(count, 3);
    let t = templates();
    assert_eq!(t.len(), 5);
    for w in &t {
        ensure_valid(w).unwrap_or_else(|e| panic!("{}: {e}", w.name));
    }
}

#[test]
fn demo_update_is_classified() {
    let dir = modules_dir().join("document-management");
    let v1: ModuleManifest = read(&dir.join("manifest.json"));
    let v2: ModuleManifest = read(&dir.join("demo-updates/1.1.0/manifest.json"));
    check_manifest(&v2, "1.0.0").unwrap();
    let pack: ViewPack = read(&dir.join("demo-updates/1.1.0/views.json"));
    assert!(pack.missing_texts(&v2.prefix).is_empty());
    let changes = diff_surfaces(&v1.contract_surface, &v2.contract_surface);
    let class_of = |name: &str| {
        let w = templates().into_iter().find(|w| w.name == name).unwrap();
        analyze(&w, &changes)
    };
    let approval = class_of("Invoice approval");
    assert_eq!(approval.impact.class, ImpactClass::ReviewRequired, "{:?}", approval.impact);
    let migrated = approval.migrated.expect("renames applied");
    assert!(migrated.step("reject").unwrap().parameters["comment"].contains("vars.comment"));
    assert!(migrated.triggers.iter().all(|t| t.suspended), "AI triggers suspended");
    let send = class_of("Send approved invoices to accounting");
    assert_eq!(send.impact.class, ImpactClass::AutoMigrated, "{:?}", send.impact);
    assert_eq!(send.migrated.unwrap().step("send").unwrap().command_id.as_deref(), Some("documents.dispatch"));
    assert_eq!(class_of("Archive a finished invoice").impact.class, ImpactClass::Broken);
    assert_eq!(class_of("Onboard a new employee").impact.class, ImpactClass::Compatible);
}
