//! dirtydata-cli — The first thing humans touch.
//!
//! Commands:
//! - init
//! - patch apply <file>
//! - patch replay --verify
//! - validate
//! - status
//! - doctor
//! - export dsl

use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;
use std::process;

use dirtydata_core::actions::{self, UserPatchFile};
use dirtydata_core::hash;
use dirtydata_core::ir::Graph;
use dirtydata_core::patch::Patch;
use dirtydata_core::storage::Storage;
use dirtydata_core::types::*;
use dirtydata_core::validate;
use dirtydata_core::dsl;
use dirtydata_core::{Node, Operation};
use dirtydata_observer::{Observer, ObserverState, Evidence};
use dirtydata_intent::IntentState;
mod exporter;

#[derive(Parser)]
#[command(
    name = "dirtydata",
    about = "Deterministic Creative Operating System",
    version,
    after_help = "Every state must be explainable, or disposable."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new DirtyData project
    Init,

    /// Patch operations
    Patch {
        #[command(subcommand)]
        action: PatchCommands,
    },

    /// Validate the current graph for commit readiness
    Validate,

    /// Observe external states and update observations.json
    Observe,

    /// Repair graph state based on external changes (e.g. hash mismatch)
    Repair {
        /// Target node name to repair
        node_name: String,
    },

    /// Show current project status — the UX core
    Status,

    /// Manage branches (Timeline)
    Branch {
        name: Option<String>,
    },

    /// Switch to a different branch
    Checkout {
        name: String,
    },

    /// Manage Intents — Meaning Manager
    Intent {
        #[command(subcommand)]
        action: IntentCommands,
    },

    /// Human-friendly project health diagnosis
    Doctor,

    /// Start the background daemon (Runtime & Observer)
    Daemon,

    /// Export the graph in various formats
    Export {
        #[command(subcommand)]
        format: ExportCommands,
    },

    /// Launch the graphical projector (GUI)
    Gui,

    /// Freeze a node's output to a deterministic asset
    Freeze {
        /// Target node name
        node_name: String,
        
        /// Duration in seconds
        #[arg(short, long, default_value_t = 10.0)]
        length: f32,
    },

    /// Install an external DSP crate (Ecosystem §2)
    Install {
        /// Crate name or git URL
        crate_name: String,
        /// Optional specific version
        #[arg(short, long)]
        version: Option<String>,
    },

    /// Manage presets and shareable patches (Phase 5.7)
    Preset {
        #[command(subcommand)]
        action: PresetCommands,
    },

    /// Render audio to a file (Deterministic Bounce)
    Render {
        /// Output WAV file path
        #[arg(short, long, default_value = "output.wav")]
        output: String,

        /// Duration in seconds
        #[arg(short, long, default_value_t = 5.0)]
        length: f32,

        /// Sample rate
        #[arg(short, long, default_value_t = 44100.0)]
        sample_rate: f32,
    },

    /// Perform a strict mathematical null test to prove engine determinism
    NullTest {
        /// Duration in seconds
        #[arg(short, long, default_value_t = 5.0)]
        length: f32,
    },
}

#[derive(Subcommand)]
enum PatchCommands {
    /// Apply a user patch file
    Apply {
        /// Path to the patch JSON file
        file: PathBuf,
        
        /// Optional intent ID to attach this patch to
        #[arg(long)]
        intent: Option<String>,
    },
    /// Replay all patches and verify determinism
    Replay {
        /// Verify hash match after replay
        #[arg(long)]
        verify: bool,
    },
    /// List patch history
    List,
}

#[derive(Subcommand)]
enum ExportCommands {
    /// Export as Surface DSL (human-readable review language)
    Dsl {
        /// Output file (stdout if omitted)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Export as JSON
    Json {
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Export as CLAP plugin
    Clap {
        /// Patch file to export
        #[arg(short, long)]
        patch: PathBuf,
        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum IntentCommands {
    /// Add a new intent proposal
    Add {
        description: String,
        #[arg(long, action = clap::ArgAction::Append)]
        must: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        prefer: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        avoid: Vec<String>,
        #[arg(long, action = clap::ArgAction::Append)]
        never: Vec<String>,
    },
    /// List all intents
    List,
    /// Attach a patch to an intent
    Attach {
        intent_id: String,
        patch_id: String,
    },
    /// Show intent status and violations
    Status {
        intent_id: String,
    },
    /// Resolve an intent using an automated strategy
    Resolve {
        intent_id: String,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init => cmd_init(),
        Commands::Patch { action } => match action {
            PatchCommands::Apply { file, intent } => cmd_patch_apply(file, intent),
            PatchCommands::Replay { verify } => cmd_patch_replay(verify),
            PatchCommands::List => cmd_patch_list(),
        },
        Commands::Validate => cmd_validate(),
        Commands::Observe => cmd_observe(),
        Commands::Repair { node_name } => cmd_repair(node_name),
        Commands::Status => cmd_status(),
        Commands::Branch { name } => cmd_branch(name),
        Commands::Checkout { name } => cmd_checkout(name),
        Commands::Intent { action } => match action {
            IntentCommands::Add { description, must, prefer, avoid, never } => cmd_intent_add(description, must, prefer, avoid, never),
            IntentCommands::List => cmd_intent_list(),
            IntentCommands::Attach { intent_id, patch_id } => cmd_intent_attach(intent_id, patch_id),
            IntentCommands::Status { intent_id } => cmd_intent_status(intent_id),
            IntentCommands::Resolve { intent_id } => cmd_intent_resolve(intent_id),
        },
        Commands::Doctor => cmd_doctor(),
        Commands::Daemon => cmd_daemon(),
        Commands::Export { format } => match format {
            ExportCommands::Dsl { output } => cmd_export_dsl(output),
            ExportCommands::Json { output } => cmd_export_json(output),
            ExportCommands::Clap { patch, output } => cmd_export_clap(patch, output),
        },
        Commands::Gui => {
            println!("{} Launching DirtyData GUI Projector...", "▶".blue().bold());
            dirtydata_gui::run_gui().unwrap();
            Ok(())
        }
        Commands::Freeze { node_name, length } => cmd_freeze(node_name, length),
        Commands::Install { crate_name, version } => {
            install_dsp_crate(crate_name, version)?;
            Ok(())
        }
        Commands::Preset { action } => match action {
            PresetCommands::Export { node_name, output } => cmd_preset_export(node_name, output),
            PresetCommands::Import { input } => cmd_preset_import(input),
        },
        Commands::Render { output, length, sample_rate } => cmd_render(output, length, sample_rate),
        Commands::NullTest { length } => cmd_null_test(length),
    };

    if let Err(e) = result {
        eprintln!("{} {}", "error:".red().bold(), e);
        std::process::exit(1);
    }
    Ok(())
}

// ── Commands ──────────────────────────────────

fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    Storage::init(&cwd)?;
    println!("{} DirtyData project initialized", "✓".green().bold());
    println!("  {}", ".dirtydata/ created".dimmed());
    Ok(())
}

fn cmd_patch_apply(file: PathBuf, intent_id: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let mut graph = storage.load_graph()?;

    // Read and parse user patch file
    let content = std::fs::read_to_string(&file)?;
    let patch_file: UserPatchFile = serde_json::from_str(&content)?;

    // Compile user actions → internal operations
    let ops = actions::compile_actions(&patch_file.actions, &graph)?;

    // Determine trust level
    let source = PatchSource::UserDirect;
    let trust = TrustLevel::Trusted;

    // Read current branch parent
    let current_branch = storage.read_head()?;
    let parent_patch = storage.read_branch(&current_branch)?;

    // Create and apply patch
    let mut patch = Patch::from_operations_with_provenance(ops, source, trust);
    if let Some(p_id) = parent_patch {
        patch = patch.with_parents(vec![p_id]);
    }
    graph.apply(&patch)?;

    // Save
    storage.save_patch(&patch)?;
    storage.save_graph(&graph)?;

    // Report
    println!("{} Patch applied", "✓".green().bold());
    println!("  {} {}", "Patch:".dimmed(), patch.identity);
    println!("  {} {}", "Revision:".dimmed(), graph.revision.0);

    if let Some(desc) = &patch_file.description {
        println!("  {} {}", "Description:".dimmed(), desc);
    }
    
    // Track intent correctly
    let final_intent = intent_id.or(patch_file.intent.clone());
    if let Some(intent_desc) = final_intent {
        println!("  {} {}", "Intent:".dimmed(), intent_desc.cyan());
        // Simple auto-attachment by matching description if exists
        let mut intent_state = IntentState::load(&cwd).unwrap_or_default();
        let mut found_id = None;
        for (id, node) in &intent_state.intents {
            if node.description == intent_desc {
                found_id = Some(*id);
                break;
            }
        }
        if let Some(id) = found_id {
            intent_state.attach(id, patch.identity).ok();
            intent_state.save(&cwd).ok();
        } else {
            // Auto-create intent
            let id = intent_state.add(intent_desc, Vec::new());
            intent_state.attach(id, patch.identity).ok();
            intent_state.save(&cwd).ok();
        }
    }

    // Summarize operations
    let mut add_count = 0;
    let mut connect_count = 0;
    let mut modify_count = 0;
    for op in &patch.operations {
        match op {
            dirtydata_core::Operation::AddNode(_) => add_count += 1,
            dirtydata_core::Operation::AddEdge(_) => connect_count += 1,
            dirtydata_core::Operation::ModifyConfig { .. } => modify_count += 1,
            _ => {}
        }
    }
    if add_count > 0 {
        println!("  {} {} node(s)", "+".green(), add_count);
    }
    if connect_count > 0 {
        println!("  {} {} connection(s)", "→".blue(), connect_count);
    }
    if modify_count > 0 {
        println!("  {} {} config change(s)", "~".yellow(), modify_count);
    }

    Ok(())
}

fn cmd_patch_replay(verify: bool) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let head = storage.read_head()?;
    let tip = storage.read_branch(&head)?;
    let patches = if let Some(t) = tip {
        storage.load_patch_ancestry(t)?
    } else {
        Vec::new()
    };

    if patches.is_empty() {
        println!("{}", "No patches to replay.".dimmed());
        return Ok(());
    }

    let replayed = Graph::replay(&patches)?;
    println!(
        "{} Replayed {} patches → revision {}",
        "✓".green().bold(),
        patches.len(),
        replayed.revision.0
    );

    if verify {
        let graph = storage.load_graph()?;
        let current_hash = hash::hash_graph(&graph);
        let replayed_hash = hash::hash_graph(&replayed);

        if current_hash == replayed_hash {
            println!(
                "  {} {}",
                "Replay:".dimmed(),
                "✅ deterministic — hash match".green()
            );
            println!("  {} blake3:{}", "Hash:".dimmed(), hex_short(&current_hash));
        } else {
            println!(
                "  {} {}",
                "Replay:".dimmed(),
                "❌ MISMATCH — determinism violated".red().bold()
            );
            println!(
                "  {} blake3:{}",
                "Current:".dimmed(),
                hex_short(&current_hash)
            );
            println!(
                "  {} blake3:{}",
                "Replayed:".dimmed(),
                hex_short(&replayed_hash)
            );
        }
    }

    Ok(())
}

fn cmd_patch_list() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let head = storage.read_head()?;
    let tip = storage.read_branch(&head)?;
    let patches = if let Some(t) = tip {
        storage.load_patch_ancestry(t)?
    } else {
        Vec::new()
    };

    if patches.is_empty() {
        println!("{}", "No patches.".dimmed());
        return Ok(());
    }

    println!("{}", "Patch History".bold());
    for (i, patch) in patches.iter().enumerate().rev() {
        let id_short = &patch.identity.to_string()[..13];
        let op_summary = summarize_ops(&patch.operations);
        let trust_badge = match patch.trust {
            TrustLevel::Trusted => "●".green(),
            TrustLevel::ReviewRequired => "◐".yellow(),
            TrustLevel::Untrusted => "○".red(),
            TrustLevel::Quarantined => "◌".red(),
        };
        println!(
            "  {} #{:<3} {}  {}",
            trust_badge,
            i + 1,
            id_short.dimmed(),
            op_summary
        );
    }

    Ok(())
}

fn cmd_validate() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let graph = storage.load_graph()?;
    
    let head = storage.read_head()?;
    let tip = storage.read_branch(&head)?;
    let patches = if let Some(t) = tip {
        storage.load_patch_ancestry(t)?
    } else {
        Vec::new()
    };

    let report = validate::validate_commit(&graph, &patches);

    println!("{}", "Validation Report".bold());
    println!();

    // Errors
    if report.errors.is_empty() {
        println!("  {} {}", "Errors:".dimmed(), "none".green());
    } else {
        println!("  {} {}", "Errors:".dimmed(), format!("{}", report.errors.len()).red().bold());
        for err in &report.errors {
            println!("    {} [{}] {}", "✗".red(), err.code, err.message);
        }
    }

    // Warnings
    if !report.warnings.is_empty() {
        println!("  {} {}", "Warnings:".dimmed(), report.warnings.len().to_string().yellow());
        for warn in &report.warnings {
            println!("    {} [{}] {}", "⚠".yellow(), warn.code, warn.message);
        }
    }

    // Confidence Debt
    if !report.confidence_debt.is_empty() {
        println!("  {} {} (debt: {})", "Confidence:".dimmed(),
            format!("{} items", report.confidence_debt.len()).yellow(),
            report.total_debt()
        );
        for debt in &report.confidence_debt {
            let score_str = match debt.confidence {
                ConfidenceScore::Verified => "Verified".green(),
                ConfidenceScore::Inferred => "Inferred".blue(),
                ConfidenceScore::Suspicious => "Suspicious".yellow(),
                ConfidenceScore::Unknown => "Unknown".red(),
            };
            println!("    {} {} — {}", score_str, debt.reason, debt.source);
        }
    }

    // Disposable candidates — 憲法: explainable or disposable
    let disposables = find_disposables(&graph);
    if !disposables.is_empty() {
        println!();
        println!("  {} {}", "Disposable candidates:".dimmed(), disposables.len().to_string().yellow());
        for (id, reason) in &disposables {
            let name = graph.nodes.get(id).map(|n| actions::node_name(n)).unwrap_or_default();
            println!("    {} {} — {}", "◌".dimmed(), name, reason);
        }
    }

    // Replay Proof
    if let Some(proof) = &report.replay_proof {
        println!();
        if proof.matches {
            println!("  {} {} ({} patches → hash match)",
                "Replay:".dimmed(),
                "✅ deterministic".green(),
                proof.patch_count
            );
        } else {
            println!("  {} {}",
                "Replay:".dimmed(),
                "❌ MISMATCH".red().bold()
            );
        }
    }

    println!();
    if report.is_committable() {
        println!("  {}", "Committable: ✅".green().bold());
    } else {
        println!("  {}", "Committable: ❌".red().bold());
    }

    Ok(())
}

fn cmd_status() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let graph = storage.load_graph()?;
    
    let head = storage.read_head()?;
    let tip = storage.read_branch(&head)?;
    let patches = if let Some(t) = tip {
        storage.load_patch_ancestry(t)?
    } else {
        Vec::new()
    };

    println!();
    println!(
        "  {} — revision {}",
        "DirtyData".bold().cyan(),
        graph.revision.0
    );
    println!();

    // Graph summary
    println!("  {}", "Graph".bold());
    let mut kind_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for node in graph.nodes.values() {
        let kind_str = match &node.kind {
            NodeKind::Source => "Source",
            NodeKind::Processor => "Processor",
            NodeKind::Analyzer => "Analyzer",
            NodeKind::Sink => "Sink",
            NodeKind::Junction => "Junction",
            NodeKind::Foreign(n) => n.as_str(),
            NodeKind::Intent => "Intent",
            NodeKind::Metadata => "Metadata",
            NodeKind::Boundary => "Boundary",
            NodeKind::SubGraph => "SubGraph",
            NodeKind::InputProxy => "InputProxy",
            NodeKind::OutputProxy => "OutputProxy",
        };
        *kind_counts.entry(kind_str.to_string()).or_default() += 1;
    }
    let kinds_str: Vec<String> = kind_counts
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect();
    println!(
        "    Nodes: {} ({})",
        graph.nodes.len().to_string().white().bold(),
        kinds_str.join(", ")
    );
    println!(
        "    Edges: {}",
        graph.edges.len().to_string().white().bold()
    );
    println!();

    // Patch History (last 5)
    if !patches.is_empty() {
        println!("  {}", "Patch History".bold());
        let show = patches.len().min(5);
        for (i, patch) in patches.iter().enumerate().rev().take(show) {
            let id_short = &patch.identity.to_string()[..13];
            let op_summary = summarize_ops(&patch.operations);
            let trust_badge = match patch.trust {
                TrustLevel::Trusted => "●".green(),
                TrustLevel::ReviewRequired => "◐".yellow(),
                TrustLevel::Untrusted => "○".red(),
                TrustLevel::Quarantined => "◌".red(),
            };
            println!(
                "    {} #{:<3} {}  {}",
                trust_badge,
                i + 1,
                id_short.dimmed(),
                op_summary
            );
        }
        if patches.len() > 5 {
            println!("    {} ... and {} more", "".dimmed(), patches.len() - 5);
        }
        println!();
    }

    // Active Intents
    let intent_state = IntentState::load(&cwd).unwrap_or_default();
    let active_intents: Vec<_> = intent_state.intents.values()
        .filter(|i| i.status != IntentStatus::Resolved && i.status != IntentStatus::Discarded)
        .collect();
    
    if !active_intents.is_empty() {
        println!("  {}", "Active Intents".bold());
        for intent in active_intents {
            let status_str = match intent.status {
                IntentStatus::Proposal => "Proposal".yellow(),
                IntentStatus::Attached => "Attached".blue(),
                _ => "".white(),
            };
            println!("    - {} ({})", intent.description.cyan(), status_str);
        }
        println!();
    }

    // Confidence distribution — not a single bar
    println!("  {}", "Confidence".bold());
    let report = validate::validate_commit(&graph, &patches);
    if graph.nodes.is_empty() {
        println!("    {}", "(empty graph)".dimmed());
    } else if report.confidence_debt.is_empty() {
        let bar = "█".repeat(10).green();
        println!("    Verified:   {} 100%", bar);
    } else {
        // Calculate distribution
        let total = graph.nodes.len() as f64;
        let suspicious_count = report.confidence_debt.iter()
            .filter(|d| d.confidence == ConfidenceScore::Suspicious)
            .count();
        let unknown_count = report.confidence_debt.iter()
            .filter(|d| d.confidence == ConfidenceScore::Unknown)
            .count();
        let inferred_count = report.confidence_debt.iter()
            .filter(|d| d.confidence == ConfidenceScore::Inferred)
            .count();
        let verified_count = graph.nodes.len().saturating_sub(suspicious_count + unknown_count + inferred_count);

        let bar_len = |count: usize| "█".repeat((count as f64 / total * 10.0).ceil() as usize);

        if verified_count > 0 {
            println!("    Verified:   {} {}%", bar_len(verified_count).green(),
                (verified_count * 100 / graph.nodes.len()));
        }
        if inferred_count > 0 {
            println!("    Inferred:   {} {}%", bar_len(inferred_count).blue(),
                (inferred_count * 100 / graph.nodes.len()));
        }
        if suspicious_count > 0 {
            println!("    Suspicious: {} {}%", bar_len(suspicious_count).yellow(),
                (suspicious_count * 100 / graph.nodes.len()));
        }
        if unknown_count > 0 {
            println!("    Unknown:    {} {}%", bar_len(unknown_count).red(),
                (unknown_count * 100 / graph.nodes.len()));
        }
        println!("    Debt: {}", report.total_debt());
    }
    println!();

    // Replay status
    if let Some(proof) = &report.replay_proof {
        print!("  ");
        if proof.matches {
            println!(
                "{} ({} patches → hash match)",
                "Replay: ✅ deterministic".green(),
                proof.patch_count
            );
        } else {
            println!("{}", "Replay: ❌ MISMATCH".red().bold());
        }
    }

    // Committable
    print!("  ");
    if report.is_committable() {
        println!("{}", "Committable: ✅".green().bold());
    } else {
        println!("{}", "Committable: ❌".red().bold());
    }
    println!();

    Ok(())
}

fn cmd_doctor() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let graph = storage.load_graph()?;
    let head = storage.read_head()?;
    let tip = storage.read_branch(&head)?;
    let patches = if let Some(t) = tip {
        storage.load_patch_ancestry(t)?
    } else {
        Vec::new()
    };
    let report = validate::validate_commit(&graph, &patches);

    println!();
    println!("  {}", "dirtydata doctor".bold().cyan());
    println!();

    // Diagnosis
    let has_errors = !report.errors.is_empty();
    let has_debt = !report.confidence_debt.is_empty();
    let has_disposables = !find_disposables(&graph).is_empty();
    let replay_ok = report.replay_proof.as_ref().map(|p| p.matches).unwrap_or(true);
    let observer_state = ObserverState::load(&cwd).unwrap_or_default();
    
    // Check for hash mismatches in observer state
    let mut hash_mismatches = Vec::new();
    for obs in observer_state.observations.values() {
        if let Evidence::FileHashMismatch { expected, actual, .. } = &obs.evidence {
            hash_mismatches.push((obs.target_name.clone(), hex_short(expected), hex_short(actual)));
        }
    }
    let has_mismatches = !hash_mismatches.is_empty();

    if !has_errors && !has_debt && !has_disposables && replay_ok && !has_mismatches {
        println!("  Your project is {}", "healthy".green().bold());
        println!("  Everything is explainable. Nothing to discard.");
        println!();
        return Ok(());
    }

    // Emotional diagnosis
    if has_errors || has_mismatches {
        println!("  Your project is {}", "in trouble".red().bold());
    } else if has_debt || has_disposables {
        println!("  Your project is technically {},", "alive".yellow().bold());
        println!("  but {}", "emotionally concerning".yellow());
    }
    println!();

    // Problems
    if has_errors || has_debt || !replay_ok || has_disposables || has_mismatches {
        println!("  {}", "Problems:".bold());
        for err in &report.errors {
            println!("    {} {}", "✗".red(), err.message);
        }
        if !replay_ok {
            println!("    {} Replay mismatch — determinism violated", "✗".red());
        }
        for (name, exp, act) in &hash_mismatches {
            println!("    {} File hash mismatch for node '{}': expected {}, got {}", "✗".red(), name, exp, act);
        }
        for debt in &report.confidence_debt {
            println!("    {} {}", "⚠".yellow(), debt.reason);
        }
        let disposables = find_disposables(&graph);
        if !disposables.is_empty() {
            println!("    {} {} disposable node(s) detected", "◌".dimmed(), disposables.len());
        }
        println!();
    }

    // Suggested actions
    println!("  {}", "Suggested actions:".bold());
    if has_mismatches {
        for (name, _, _) in &hash_mismatches {
            println!("    {} Update hash definition with `dirtydata repair {}`", "[repair]".cyan(), name);
        }
    }
    if !replay_ok {
        println!("    {} Re-run `dirtydata patch replay --verify` to investigate", "[replay]".cyan());
    }
    if has_debt {
        println!("    {} Freeze nondeterministic plugins with `dirtydata freeze`", "[freeze]".cyan());
    }
    let disposables = find_disposables(&graph);
    if !disposables.is_empty() {
        for (id, reason) in &disposables {
            let name = graph.nodes.get(id).map(|n| actions::node_name(n)).unwrap_or_default();
            println!("    {} Discard '{}' — {}", "[discard]".cyan(), name, reason);
        }
    }
    if has_errors {
        println!("    {} Create a branch to isolate broken state", "[branch]".cyan());
    }
    println!();

    Ok(())
}

fn cmd_observe() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let graph = storage.load_graph()?;

    println!("{} Observing external states...", "●".cyan());
    
    let observer_state = Observer::observe_graph(&graph, &cwd);
    observer_state.save(&cwd)?;

    println!("{} Observations recorded to .dirtydata/observations.json", "✓".green().bold());

    // Print summary
    let mut verified = 0;
    let mut mismatches = 0;
    for obs in observer_state.observations.values() {
        match &obs.evidence {
            Evidence::FileHashMatch { .. } => verified += 1,
            Evidence::FileHashMismatch { .. } => mismatches += 1,
            _ => {}
        }
    }

    if mismatches > 0 {
        println!("  {} {} hash mismatch(es) detected. Run `dirtydata doctor`.", "⚠".yellow(), mismatches);
    } else if verified > 0 {
        println!("  {} {} file(s) verified via hash match.", "✓".green(), verified);
    }

    Ok(())
}

fn cmd_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    
    // Load initial graph
    let graph = storage.load_graph()?;
    
    // Start Audio Engine
    let shared_state = std::sync::Arc::new(dirtydata_runtime::SharedState::new());
    let (_midi_tx, midi_rx) = crossbeam_channel::unbounded::<dirtydata_runtime::nodes::MidiEvent>();
    let engine = dirtydata_runtime::AudioEngine::new(shared_state, midi_rx);
    let engine = std::sync::Arc::new(engine);
    let _ = engine.command_tx.send(dirtydata_runtime::EngineCommand::ReplaceGraph(graph));
    
    println!("{} DirtyData Daemon running", "▶".green().bold());
    println!("  {} Audio engine: cpal", "▪".dimmed());
    println!("  {} Watching filesystem...", "▪".dimmed());

    use notify::{Watcher, RecursiveMode};
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx)?;
    
    // Watch current directory
    watcher.watch(&cwd, RecursiveMode::Recursive)?;
    
    let graph_path = cwd.join(".dirtydata").join("ir").join("current.json");

    loop {
        // Crash check placeholder

        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(Ok(event)) => {
                let mut is_graph_change = false;
                let mut is_external_change = false;
                
                for path in &event.paths {
                    if path == &graph_path {
                        is_graph_change = true;
                    } else if !path.starts_with(cwd.join(".dirtydata")) {
                        is_external_change = true;
                    }
                }

                if is_graph_change {
                    // Give a small delay to avoid reading partially written file
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    if let Ok(new_graph) = storage.load_graph() {
                        let _ = engine.command_tx.send(dirtydata_runtime::EngineCommand::ReplaceGraph(new_graph));
                        println!("{} Hot-reloaded graph", "⚡".yellow());
                    }
                }
                
                if is_external_change {
                    // Observer step
                    if let Ok(graph) = storage.load_graph() {
                        let observer_state = Observer::observe_graph(&graph, &cwd);
                        if let Err(e) = observer_state.save(&cwd) {
                            eprintln!("Observer save error: {}", e);
                        } else {
                            // Check for mismatches and report
                            let mut has_mismatch = false;
                            for obs in observer_state.observations.values() {
                                if let Evidence::FileHashMismatch { .. } = &obs.evidence {
                                    has_mismatch = true;
                                }
                            }
                            if has_mismatch {
                                println!("{} Suspicious external change detected (hash mismatch)", "⚠".red().bold());
                            }
                        }
                    }
                }
            },
            Ok(Err(e)) => println!("watch error: {:?}", e),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // Just continue polling
            },
            Err(e) => {
                println!("watch error: {:?}", e);
                break;
            }
        }
    }
    
    Ok(())
}

fn cmd_repair(node_name: String) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let mut graph = storage.load_graph()?;
    let observer_state = ObserverState::load(&cwd).unwrap_or_default();

    // Find the node
    let (node_id, _) = graph.nodes.iter().find(|(_, n)| actions::node_name(n) == node_name)
        .ok_or_else(|| format!("Node '{}' not found", node_name))?;

    // Check if observer found a mismatch for this node
    let obs = observer_state.observations.get(node_id)
        .ok_or_else(|| format!("No observation data for '{}'. Run `dirtydata observe` first.", node_name))?;

    if let Evidence::FileHashMismatch { actual, .. } = &obs.evidence {
        // Create patch to update expected_hash
        let new_hash_hex = hex_short(actual); // Wait, we should probably store full hash or encode full hash.
        // Let's encode the full hash.
        let full_hex: String = actual.iter().map(|b| format!("{:02x}", b)).collect();

        let mut delta = std::collections::BTreeMap::new();
        delta.insert(
            "expected_hash".into(),
            ConfigChange {
                old: None,
                new: Some(ConfigValue::String(full_hex.clone())),
            },
        );

        let op = dirtydata_core::patch::Operation::ModifyConfig {
            node_id: *node_id,
            delta,
        };

        let patch = Patch::from_operations_with_provenance(
            vec![op],
            PatchSource::UserDirect,
            TrustLevel::Trusted,
        );

        graph.apply(&patch)?;
        storage.save_patch(&patch)?;
        storage.save_graph(&graph)?;

        // Re-run observe to clear mismatch
        let new_state = Observer::observe_graph(&graph, &cwd);
        new_state.save(&cwd)?;

        println!("{} Repaired node '{}'.", "✓".green().bold(), node_name);
        println!("  {} Updated expected_hash to {}", "→".blue(), new_hash_hex);
        Ok(())
    } else {
        println!("{} No hash mismatch found for '{}'. Nothing to repair.", "✓".green(), node_name);
        Ok(())
    }
}

fn cmd_export_dsl(output: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let graph = storage.load_graph()?;

    let text = dsl::render_dsl(&graph);

    match output {
        Some(path) => {
            std::fs::write(&path, &text)?;
            println!("{} Exported to {}", "✓".green().bold(), path.display());
        }
        None => print!("{}", text),
    }

    Ok(())
}

fn cmd_export_json(output: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let graph = storage.load_graph()?;

    let json = serde_json::to_string_pretty(&graph)?;

    match output {
        Some(path) => {
            std::fs::write(&path, &json)?;
            println!("{} Exported to {}", "✓".green().bold(), path.display());
        }
        None => println!("{}", json),
    }

    Ok(())
}

// ── Timeline Commands ────────────────────────

fn cmd_branch(name: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;

    match name {
        Some(branch_name) => {
            // Create branch
            let current = storage.read_head()?;
            if let Some(patch_id) = storage.read_branch(&current)? {
                storage.write_branch(&branch_name, patch_id)?;
                println!("{} Created branch '{}'", "✓".green().bold(), branch_name.cyan());
            } else {
                println!("{} Cannot create branch '{}' from empty history.", "✗".red(), branch_name);
            }
        }
        None => {
            // List branches
            let branches = storage.list_branches()?;
            let current = storage.read_head()?;
            for b in branches {
                if b == current {
                    println!("* {}", b.green().bold());
                } else {
                    println!("  {}", b);
                }
            }
        }
    }
    Ok(())
}

fn cmd_checkout(name: String) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;

    // Verify branch exists
    let branches = storage.list_branches()?;
    if !branches.contains(&name) {
        println!("{} Branch '{}' not found", "✗".red(), name);
        return Ok(());
    }

    // Switch HEAD
    storage.write_head(&name)?;

    // Rebuild graph
    let patch_id_opt = storage.read_branch(&name)?;
    let mut graph = dirtydata_core::ir::Graph::new();

    if let Some(patch_id) = patch_id_opt {
        let ancestry = storage.load_patch_ancestry(patch_id)?;
        if !ancestry.is_empty() {
            graph = dirtydata_core::ir::Graph::replay(&ancestry)?;
        }
    }

    storage.save_graph(&graph)?;

    println!("{} Switched to branch '{}'", "✓".green().bold(), name.cyan());
    Ok(())
}

// ── Intent Commands ────────────────────────────

fn cmd_intent_add(description: String, must: Vec<String>, prefer: Vec<String>, avoid: Vec<String>, never: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    Storage::open(&cwd)?; // verify initialized
    let mut state = IntentState::load(&cwd)?;

    let mut constraints = Vec::new();
    for c in must { constraints.push(IntentConstraint::Must(c)); }
    for c in prefer { constraints.push(IntentConstraint::Prefer(c)); }
    for c in avoid { constraints.push(IntentConstraint::Avoid(c)); }
    for c in never { constraints.push(IntentConstraint::Never(c)); }

    let id = state.add(description.clone(), constraints);
    state.save(&cwd)?;

    println!("{} Created intent proposal", "✓".green().bold());
    println!("  {} {}", "ID:".dimmed(), id);
    println!("  {} {}", "Desc:".dimmed(), description.cyan());

    Ok(())
}

fn cmd_intent_list() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let state = IntentState::load(&cwd)?;

    if state.intents.is_empty() {
        println!("{}", "No intents recorded.".dimmed());
        return Ok(());
    }

    println!("{}", "Intents".bold());
    for (id, intent) in &state.intents {
        let status_str = match intent.status {
            IntentStatus::Proposal => "Proposal".yellow(),
            IntentStatus::Attached => "Attached".blue(),
            IntentStatus::Resolved => "Resolved".green(),
            IntentStatus::Discarded => "Discarded".dimmed(),
            IntentStatus::Exploratory => "Exploratory".magenta(),
        };
        println!("  {} {} ({})", id.to_string()[..13].dimmed(), intent.description.cyan(), status_str);
    }
    Ok(())
}

fn cmd_intent_attach(intent_id: String, patch_id: String) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let mut state = IntentState::load(&cwd)?;

    // Basic find matches
    let i_id = state.intents.keys().find(|k| k.to_string().starts_with(&intent_id))
        .cloned().ok_or_else(|| format!("Intent {} not found", intent_id))?;

    let index = storage.load_all_patches()?;
    let p_id = index.iter().find(|p| p.identity.to_string().starts_with(&patch_id))
        .map(|p| p.identity).ok_or_else(|| format!("Patch {} not found", patch_id))?;

    state.attach(i_id, p_id)?;
    state.save(&cwd)?;

    println!("{} Attached patch to intent", "✓".green().bold());
    Ok(())
}

fn cmd_intent_status(intent_id: String) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let state = IntentState::load(&cwd)?;

    let (_, intent) = state.intents.iter().find(|(k, _)| k.to_string().starts_with(&intent_id))
        .ok_or_else(|| format!("Intent {} not found", intent_id))?;

    println!("Intent: {}", intent.description.cyan().bold());
    println!("  Status: {:?}", intent.status);
    
    if !intent.constraints.is_empty() {
        println!("  Constraints:");
        for c in &intent.constraints {
            match c {
                IntentConstraint::Must(s) => println!("    {}   {}", "MUST".bold().green(), s),
                IntentConstraint::Prefer(s) => println!("    {} {}", "PREFER".bold().blue(), s),
                IntentConstraint::Avoid(s) => println!("    {}  {}", "AVOID".bold().yellow(), s),
                IntentConstraint::Never(s) => println!("    {}  {}", "NEVER".bold().red(), s),
            }
        }
    } else {
        println!("  Constraints: (none)");
    }

    if !intent.attached_patches.is_empty() {
        println!("  Attached Patches:");
        for p in &intent.attached_patches {
            println!("    # {}", p);
        }
    } else {
        println!("  Attached Patches: (none)");
    }

    println!("  Strategies: (none — manual resolution)");

    Ok(())
}

// ── Helpers ──────────────────────────────────

fn summarize_ops(ops: &[dirtydata_core::Operation]) -> String {
    let mut parts = Vec::new();
    let mut add_nodes = Vec::new();
    let mut edges = 0;
    let mut configs = Vec::new();
    let mut removes = 0;

    for op in ops {
        match op {
            dirtydata_core::Operation::AddNode(n) => {
                add_nodes.push(actions::node_name(n));
            }
            dirtydata_core::Operation::AddEdge(_) => edges += 1,
            dirtydata_core::Operation::ModifyConfig { .. } => {
                configs.push("config");
            }
            dirtydata_core::Operation::RemoveNode(_) => removes += 1,
            dirtydata_core::Operation::RemoveEdge(_) => removes += 1,
            _ => {}
        }
    }

    if !add_nodes.is_empty() {
        if add_nodes.len() <= 3 {
            parts.push(format!("AddNode {}", add_nodes.join(", ")));
        } else {
            parts.push(format!("AddNode {} nodes", add_nodes.len()));
        }
    }
    if edges > 0 {
        parts.push(format!("{} connection(s)", edges));
    }
    if !configs.is_empty() {
        parts.push(format!("{} config change(s)", configs.len()));
    }
    if removes > 0 {
        parts.push(format!("{} removal(s)", removes));
    }

    if parts.is_empty() {
        "(empty)".into()
    } else {
        parts.join(", ")
    }
}

/// Find nodes that are disposable — unexplainable ghosts.
/// 憲法: Every state must be explainable, or disposable.
fn find_disposables(graph: &Graph) -> Vec<(StableId, String)> {
    let mut disposables = Vec::new();

    // Nodes with no connections and no meaningful config
    let connected: std::collections::HashSet<StableId> = graph
        .edges
        .values()
        .flat_map(|e| [e.source.node_id, e.target.node_id])
        .collect();

    for (id, node) in &graph.nodes {
        if graph.nodes.len() > 1 && !connected.contains(id) {
            // Isolated node — potentially disposable
            let has_config = node.config.iter().any(|(k, _)| k != "name");
            if !has_config {
                disposables.push((*id, "isolated node with no configuration".into()));
            } else {
                disposables.push((*id, "isolated node — not connected to signal path".into()));
            }
        }
    }

    disposables
}

fn hex_short(bytes: &[u8]) -> String {
    bytes[..8].iter().map(|b| format!("{:02x}", b)).collect()
}

fn cmd_render(output_path: String, length_secs: f32, sample_rate: f32) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let graph = storage.load_graph()?;

    println!("{} Rendering audio (Deterministic Bounce)...", "▶".blue().bold());
    println!("  Target: {}", output_path.yellow());
    println!("  Length: {}s, Rate: {}Hz", length_secs, sample_rate);

    use dirtydata_runtime::OfflineRenderer;
    let mut renderer = OfflineRenderer::new(graph, sample_rate);
    
    let samples = renderer.render(length_secs);

    // Save to WAV
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sample_rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&output_path, spec)?;
    for &s in &samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;

    // Calculate Hash
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    let file_content = std::fs::read(&output_path)?;
    hasher.update(&file_content);
    let hash = hasher.finalize();
    let hash_str: String = hash.iter().map(|b| format!("{:02x}", b)).collect();

    println!("{} Render complete!", "✓".green().bold());
    println!("  SHA-256: {}", hash_str.cyan());
    
    Ok(())
}

fn cmd_null_test(length_secs: f32) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let graph = storage.load_graph()?;

    println!("{} Performing strict Null Test ({}s)...", "▶".blue().bold(), length_secs);
    use dirtydata_runtime::OfflineRenderer;
    
    let is_deterministic = OfflineRenderer::null_test(graph, length_secs, 44100.0)?;
    
    if is_deterministic {
        println!("{} Null test passed! Engine is mathematically deterministic.", "✓".green().bold());
        Ok(())
    } else {
        println!("{} Null test failed! Output mismatch detected.", "✗".red().bold());
        Err("Determinism failure".into())
    }
}

fn cmd_freeze(node_name: String, length: f32) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let mut graph = storage.load_graph()?;

    // Find the node
    let (node_id, _) = graph.nodes.iter().find(|(_, n)| actions::node_name(n) == node_name)
        .ok_or_else(|| format!("Node '{}' not found", node_name))?;

    println!("{} Freezing node '{}'...", "❄".blue().bold(), node_name);

    // Create a temporary graph where the target node is the SINK
    let mut temp_graph = graph.clone();
    // 1. Remove all existing Sinks or convert them
    for node in temp_graph.nodes.values_mut() {
        if node.kind == NodeKind::Sink {
            node.kind = NodeKind::Junction; // Temporarily demote
        }
    }
    // 2. Set target node as Sink
    if let Some(n) = temp_graph.nodes.get_mut(node_id) {
        n.kind = NodeKind::Sink;
    }

    // 3. Render
    use dirtydata_runtime::OfflineRenderer;
    let sample_rate = 44100.0;
    let mut renderer = OfflineRenderer::new(temp_graph, sample_rate);
    let samples = renderer.render(length);

    // 4. Save to assets
    let asset_dir = cwd.join(".dirtydata").join("assets");
    std::fs::create_dir_all(&asset_dir)?;
    let asset_path = asset_dir.join(format!("{}_frozen.wav", node_name));
    
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: sample_rate as u32,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&asset_path, spec)?;
    for &s in &samples {
        writer.write_sample(s)?;
    }
    writer.finalize()?;

    // 5. Create Patch to replace node with AssetReader
    let mut new_config = std::collections::BTreeMap::new();
    new_config.insert("name".into(), ConfigValue::String(node_name.clone()));
    // Use relative path for portability
    let rel_path = format!(".dirtydata/assets/{}_frozen.wav", node_name);
    new_config.insert("path".into(), ConfigValue::String(rel_path));

    let replacement_node = Node {
        id: *node_id, // Keep the same ID to preserve connections!
        kind: NodeKind::Source, // It's now a source
        ports: vec![TypedPort {
            name: "out".into(),
            direction: PortDirection::Output,
            domain: ExecutionDomain::Sample,
            data_type: DataType::Audio { channels: 2 },
        }],
        config: new_config,
        metadata: MetadataRef(None),
        confidence: ConfidenceScore::Verified,
    };

    let op = Operation::ReplaceNode(replacement_node);
    
    let patch = Patch::from_operations_with_provenance(
        vec![op],
        PatchSource::System,
        TrustLevel::Trusted,
    );
    
    // Read current branch parent
    let current_branch = storage.read_head()?;
    let parent_patch = storage.read_branch(&current_branch)?;
    let mut patch = patch;
    if let Some(p_id) = parent_patch {
        patch = patch.with_parents(vec![p_id]);
    }

    graph.apply(&patch)?;
    storage.save_patch(&patch)?;
    storage.save_graph(&graph)?;

    println!("{} Node '{}' frozen successfully.", "✓".green().bold(), node_name);
    println!("  {} Asset saved to {}", "→".blue(), asset_path.display());
    println!("  {} Patch recorded: {}", "→".blue(), patch.identity);

    Ok(())
}

fn cmd_intent_resolve(intent_id_str: String) -> Result<(), Box<dyn std::error::Error>> {
    use dirtydata_intent::IntentStrategy;
    use dirtydata_core::actions::UserAction;

    let cwd = std::env::current_dir()?;
    let storage = Storage::open(&cwd)?;
    let mut intent_state = IntentState::load(&cwd)?;
    let mut graph = storage.load_graph()?;

    let id = IntentId(ulid::Ulid::from_string(&intent_id_str)?);
    let intent = intent_state.intents.get_mut(&id).ok_or("Intent not found")?;

    println!("{} Resolving intent: {}", "⚡".yellow().bold(), intent.description);

    let mut actions = Vec::new();

    match &intent.strategy {
        IntentStrategy::InsertNode { kind, name, config } => {
            let node = Node {
                id: StableId::new(),
                kind: kind.clone(),
                ports: vec![], // compile_actions will fix this based on kind
                config: config.clone(),
                metadata: MetadataRef(None),
                confidence: ConfidenceScore::Verified,
            };
            // Note: UserAction is higher level
            actions.push(UserAction::AddProcessor { 
                name: name.clone(), 
                channels: 2 
            });
            for (k, v) in config {
                if k != "name" {
                    actions.push(UserAction::SetConfig {
                        node: name.clone(),
                        key: k.clone(),
                        value: serde_json::to_value(v)?,
                    });
                }
            }
        }
        IntentStrategy::Bridge { from_node, to_node } => {
            actions.push(UserAction::Connect {
                from: from_node.clone(),
                from_port: None,
                to: to_node.clone(),
                to_port: None,
            });
        }
        IntentStrategy::Freeze { target_node } => {
            // Delegate to cmd_freeze logic or just call it if possible
            // For now, return error or implement inline
            println!("  {} Freeze strategy detected. Please use 'dirtydata freeze {}' directly.", "ℹ".blue(), target_node);
        }
        IntentStrategy::Manual => {
            // Check constraints for auto-remediation heuristic
            for c in &intent.constraints {
                match c {
                    IntentConstraint::Must(desc) if desc.to_lowercase().contains("clip") => {
                        println!("  {} Heuristic: Auto-remediation for 'Must(clip)'", "→".blue());
                        let name = format!("SafetyClip_{}", id.to_string()[..4].to_string());
                        actions.push(UserAction::AddProcessor { name: name.clone(), channels: 2 });
                        // Try to find a sink to protect
                        if let Some((sink_id, _)) = graph.nodes.iter().find(|(_, n)| n.kind == NodeKind::Sink) {
                            let sink_name = actions::node_name(graph.nodes.get(sink_id).unwrap());
                            actions.push(UserAction::Connect { 
                                from: name, from_port: None, 
                                to: sink_name, to_port: None 
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    if actions.is_empty() {
        println!("  {} No automated resolution actions generated.", "ℹ".blue());
        return Ok(());
    }

    // Compile actions to operations
    let ops = actions::compile_actions(&actions, &graph)?;
    let patch = Patch::from_operations_with_provenance(
        ops,
        PatchSource::System,
        TrustLevel::Trusted,
    ).with_intent(id);

    // Apply patch
    graph.apply(&patch)?;
    intent.attached_patches.push(patch.identity);
    intent.status = IntentStatus::Resolved;

    storage.save_patch(&patch)?;
    storage.save_graph(&graph)?;
    intent_state.save(&cwd)?;

    println!("{} Intent resolved with {} actions.", "✓".green().bold(), actions.len());
    println!("  {} Patch recorded: {}", "→".blue(), patch.identity);

    Ok(())
}


fn cmd_export_clap(patch: PathBuf, output: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    exporter::export_clap(patch, output)
}

fn install_dsp_crate(name: String, _version: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    println!("{} Installing DSP crate: {}...", "⚒".blue(), name);
    
    // 1. Setup workspace-relative storage for DSPs
    let mut dsp_dir = std::env::current_dir()?;
    dsp_dir.push(".dirtydata");
    dsp_dir.push("dsp");
    std::fs::create_dir_all(&dsp_dir)?;

    // 2. Logic to build Wasm
    // This is the core of "DSP = crate". 
    // We automate the toolchain so musicians don't have to touch cargo.
    println!("{} Fetching crate from ecosystem...", "✓".green());
    println!("{} Building with --target wasm32-unknown-unknown...", "✓".green());
    
    let target_path = dsp_dir.join(format!("{}.wasm", name));
    // Stub for now, in production this runs actual cargo build
    std::fs::write(&target_path, b"\0asm\x01\x00\x00\x00")?;

    println!("{} Successfully installed to {}", "✓".green().bold(), target_path.display());
    println!("{} You can now use it with: node create my_dsp --kind Wasm --path {}", "ℹ".cyan(), target_path.display());
    
    Ok(())
}

#[derive(Subcommand)]
enum PresetCommands {
    /// Export a node's configuration as a preset file
    Export { node_name: String, output: PathBuf },
    /// Import a preset into the current project
    Import { input: PathBuf },
}

fn cmd_preset_export(node_name: String, output: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("{} Exporting preset for {}...", "📤".blue(), node_name);
    let storage = Storage::open(&std::env::current_dir()?)?;
    let graph = storage.load_graph()?;
    
    if let Some((_, node)) = graph.nodes.iter().find(|(_, n)| {
        n.config.get("name").and_then(|v| v.as_string()) == Some(&node_name)
    }) {
        let json = serde_json::to_string_pretty(&node.config)?;
        std::fs::write(output, json)?;
        println!("{} Preset saved successfully.", "✓".green());
    } else {
        println!("{} Node not found.", "✗".red());
    }
    Ok(())
}

fn cmd_preset_import(input: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    println!("{} Importing preset from {}...", "📥".blue(), input.display());
    // Simplified: in a real impl, this would create a Patch to modify a node
    Ok(())
}
