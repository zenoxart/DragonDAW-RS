//! `dragon` – headless Dragon DAW tools.
//!
//! ```text
//! dragon render <project.dragon> <out.wav|out.flac|folder> [--format wav16|wav24|wav32|flac16|flac24]
//!               [--rate 48000] [--lufs -14] [--peak -1] [--stems] [--mono] [--no-plugins]
//! dragon info <project.dragon>          tracks, patterns, length, missing files
//! dragon new <project.dragon>           writes an empty project
//! dragon loudness <audio file>          integrated LUFS, true peak, sample peak
//! dragon analyze <audio file>           BPM and key detection
//! dragon scan [folder …]                scans plugins (each file in its own process)
//! dragon scan-plugin <clap|vst3> <path> internal: prints the descriptors of one file as JSON
//! ```

use dragon_engine::export::{export_with, ExportFormat, ExportOptions};
use dragon_engine::model::{PluginFormat, Project};
use dragon_engine::render::Pool;
use dragon_engine::samples::{decode, BitDepth, SampleCache};
use dragon_engine::{analysis, dsp, plugins, project_io};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  dragon render <project> <out> [--format wav16|wav24|wav32|flac16|flac24] [--rate N] [--lufs X] [--peak X] [--stems] [--mono] [--no-plugins]
  dragon info <project>
  dragon new <project>
  dragon loudness <audio>
  dragon analyze <audio>
  dragon scan [folder ...]
  dragon scan-plugin <clap|vst3> <path>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(String::as_str) {
        Some("render") => render(&args[1..]),
        Some("info") => info(&args[1..]),
        Some("new") => new(&args[1..]),
        Some("loudness") => loudness(&args[1..]),
        Some("analyze") => analyze(&args[1..]),
        Some("scan") => scan(&args[1..]),
        Some("scan-plugin") => scan_plugin(&args[1..]),
        Some("-h" | "--help" | "help") | None => {
            println!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command '{other}'\n{USAGE}")),
    };
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn arg<'a>(args: &'a [String], i: usize, what: &str) -> Result<&'a str, String> {
    args.get(i).map(String::as_str).ok_or_else(|| format!("missing {what}\n{USAGE}"))
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn load(path: &str) -> Result<(Project, SampleCache), String> {
    let path = Path::new(path);
    let p = project_io::load(path).map_err(|e| e.to_string())?;
    let cache = SampleCache::default();
    cache.set_base(path.parent().map(Path::to_path_buf));
    Ok((p, cache))
}

fn render(args: &[String]) -> Result<(), String> {
    let (p, cache) = load(arg(args, 0, "project")?)?;
    let out = PathBuf::from(arg(args, 1, "output")?);
    let mut o = ExportOptions::default();
    let fmt = flag_value(args, "--format").unwrap_or_else(|| match out.extension().and_then(|e| e.to_str()) {
        Some("flac") => "flac24".into(),
        _ => "wav24".into(),
    });
    o.format = match fmt.as_str() {
        "wav16" => ExportFormat::Wav(BitDepth::Int16),
        "wav24" => ExportFormat::Wav(BitDepth::Int24),
        "wav32" => ExportFormat::Wav(BitDepth::Float32),
        "flac16" => ExportFormat::Flac16,
        "flac24" => ExportFormat::Flac24,
        f => return Err(format!("unknown format '{f}'")),
    };
    if let Some(r) = flag_value(args, "--rate") {
        o.rate = r.parse().map_err(|_| "bad --rate")?;
        o.render_rate = o.rate;
    }
    if let Some(x) = flag_value(args, "--lufs") {
        o.loudness_target = Some(x.parse().map_err(|_| "bad --lufs")?);
    }
    if let Some(x) = flag_value(args, "--peak") {
        o.normalize_peak = Some(x.parse().map_err(|_| "bad --peak")?);
    }
    o.stems = args.iter().any(|a| a == "--stems");
    o.mono = args.iter().any(|a| a == "--mono");
    let use_plugins = !args.iter().any(|a| a == "--no-plugins");
    let missing = project_io::missing_files(&p);
    for m in &missing {
        eprintln!("warning: missing file {}", m.display());
    }
    let mut last = -1i32;
    let report = export_with(
        &p,
        &cache,
        &out,
        &o,
        &mut || if use_plugins { Pool::with_plugins(Box::new(plugins::Host)) } else { Pool::new() },
        &mut |x| {
            let pc = (x * 100.0) as i32;
            if pc / 10 != last / 10 {
                eprint!("\rrendering {pc:3}%");
                last = pc;
            }
        },
    )
    .map_err(|e| e.to_string())?;
    eprintln!("\rrendering 100%");
    for f in &report.files {
        println!("{}", f.display());
    }
    println!("{:.2} s, {:.1} LUFS, {:.1} dBTP", report.seconds, report.loudness.integrated, report.loudness.true_peak_db);
    Ok(())
}

fn info(args: &[String]) -> Result<(), String> {
    let (p, _) = load(arg(args, 0, "project")?)?;
    println!("{}", p.meta.name);
    println!("tempo {:.2} BPM, length {:.1} beats", p.bpm, p.song_length());
    println!("{} tracks, {} rack channels, {} patterns, {} mixer channels", p.tracks.len(), p.channels.len(), p.patterns.len(), p.mixer.channels.len());
    for t in &p.tracks {
        println!("  track '{}': {} clips", t.name, t.clips.len());
    }
    let missing = project_io::missing_files(&p);
    if !missing.is_empty() {
        println!("missing files:");
        for m in missing {
            println!("  {}", m.display());
        }
    }
    Ok(())
}

fn new(args: &[String]) -> Result<(), String> {
    let path = PathBuf::from(arg(args, 0, "project")?);
    let mut p = Project::new_default();
    p.meta.name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    project_io::save(&p, &path, 0).map_err(|e| e.to_string())?;
    println!("{}", path.display());
    Ok(())
}

fn loudness(args: &[String]) -> Result<(), String> {
    let d = decode(Path::new(arg(args, 0, "audio file")?)).map_err(|e| e.to_string())?;
    let s = dsp::loudness::measure(&d.l, &d.r, d.rate as f32);
    println!("integrated {:.1} LUFS\ntrue peak  {:.1} dBTP\nsample peak {:.1} dBFS\nlength {:.2} s", s.integrated, s.true_peak_db, s.sample_peak_db, d.duration());
    Ok(())
}

fn analyze(args: &[String]) -> Result<(), String> {
    let d = decode(Path::new(arg(args, 0, "audio file")?)).map_err(|e| e.to_string())?;
    let a = analysis::analyze(&d);
    println!("{}", serde_json::to_string_pretty(&a).map_err(|e| e.to_string())?);
    Ok(())
}

fn scan(args: &[String]) -> Result<(), String> {
    let mut roots: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if roots.is_empty() {
        roots = plugins::default_paths();
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut all = Vec::new();
    for (fmt, path) in plugins::find_plugin_files(&roots) {
        match plugins::scan_isolated(&exe, fmt, &path, std::time::Duration::from_secs(20)) {
            Ok(list) => all.extend(list),
            Err(e) => eprintln!("skipped: {e}"),
        }
    }
    for p in &all {
        println!("{:5} {:<32} {:<20} {}", if p.instrument { "inst" } else { "fx" }, p.name, p.vendor, p.path.display());
    }
    eprintln!("{} plugins", all.len());
    Ok(())
}

fn scan_plugin(args: &[String]) -> Result<(), String> {
    let fmt = match arg(args, 0, "format")? {
        "clap" => PluginFormat::Clap,
        "vst3" => PluginFormat::Vst3,
        f => return Err(format!("unknown format '{f}'")),
    };
    let list = plugins::scan_file(fmt, Path::new(arg(args, 1, "path")?)).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string(&list).map_err(|e| e.to_string())?);
    Ok(())
}
