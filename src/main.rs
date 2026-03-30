use chrono::{Local, TimeZone};
use clap::{Parser, Subcommand};
use colored::*;
use crossterm::{cursor, terminal, ExecutableCommand, QueueableCommand};
use humansize::{format_size, DECIMAL};
use notify::{EventKind, RecursiveMode, Watcher};
use ignore::gitignore::GitignoreBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{stdout, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant, UNIX_EPOCH};
use unicode_width::UnicodeWidthStr;

#[derive(clap::ValueEnum, Clone, Debug, Default)]
enum SortBy {
    #[default]
    Size,
    Name,
}

#[derive(Parser, Debug)]
#[command(
    author, 
    version, 
    about = "A fast, highly visual directory analysis tool.", 
    long_about = "Bart is a temporal filesystem profiler with interactive filtering, semantic code grouping, and beautiful emoji-based terminal output."
)]
struct Args {
    #[arg(default_value = ".", help = "The directory path to scan")]
    path: PathBuf,

    #[arg(short, long, default_value_t = 1, help = "Maximum depth to display")]
    depth: usize,

    #[arg(short = 'n', long, default_value_t = 0, help = "Number of top entries to show per directory (0 for all)")]
    limit: usize,

    #[arg(short, long, value_enum, default_value_t = SortBy::Size, help = "Sort by size or name")]
    sort: SortBy,

    #[arg(short, long, help = "Watch directory for live updates")]
    watch: bool,

    #[arg(short = 'f', long, help = "Launch the interactive TUI to filter by file format and perform actions")]
    filter: bool,

    #[arg(long, help = "Do not respect .gitignore rules and include ignored directories (.git, node_modules, target)")]
    no_ignore: bool,

    #[arg(long, help = "Export the entire directory structure to JSON format")]
    json: bool,

    #[arg(long, help = "Export the entire directory structure to CSV format")]
    csv: bool,

    #[arg(long, help = "Compare the current directory against the previous scan, displaying a differential size breakdown")]
    diff: bool,

    #[arg(long, help = "Perform a deep semantic breakdown showing exactly why a directory is large")]
    explain: bool,

    #[arg(long, help = "Show the top N largest individual files globally")]
    top: Option<usize>,

    #[arg(long, help = "Calculate and display bloat score and auto-insights")]
    insights: bool,

    #[arg(long, help = "Identify known disposable heavyweights (e.g. target, node_modules) for safe cleanup (dry-run by default)")]
    clean: bool,

    #[arg(long, help = "Actually delete the directories/files identified by --clean")]
    apply: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Manage tracked directories for the daemon")]
    Index {
        #[command(subcommand)]
        action: IndexCommands,
    },
    #[command(about = "Manage the background observability daemon")]
    Daemon {
        #[command(subcommand)]
        action: DaemonCommands,
    },
}

#[derive(Subcommand, Debug)]
enum IndexCommands {
    #[command(about = "Add a directory to the watch list")]
    Add { path: PathBuf },
    #[command(about = "Remove a directory from the watch list")]
    Remove { path: PathBuf },
    #[command(about = "List all tracked directories")]
    List,
}

#[derive(Subcommand, Debug)]
enum DaemonCommands {
    #[command(about = "Start the observability daemon")]
    Start,
    #[command(about = "Check daemon status")]
    Status,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Node {
    path: PathBuf,
    size: u64,
    file_count: usize,
    is_dir: bool,
    children: Vec<Node>,
    depth: usize,
    #[serde(default)]
    modified: u64,
}

impl Node {
    fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .to_string()
    }

    fn emoji(&self) -> String {
        if self.is_dir {
            "📁".to_string()
        } else {
            if let Some(ext) = self.path.extension().and_then(|s| s.to_str()) {
                ext_to_emoji(&ext.to_lowercase()).unwrap_or("📄").to_string()
            } else {
                "📄".to_string()
            }
        }
    }

    fn emoji_summaries(&self) -> Vec<String> {
        if self.is_dir {
            let mut emojis = HashMap::new();
            for child in &self.children {
                Self::collect_emojis(child, &mut emojis);
            }
            if emojis.is_empty() {
                vec![]
            } else {
                let mut sorted_emojis: Vec<_> = emojis.into_iter().collect();
                sorted_emojis.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
                sorted_emojis
                    .into_iter()
                    .map(|(e, count)| format!("{} {} ({})", count, e, emoji_to_def(e)))
                    .collect()
            }
        } else {
            vec![]
        }
    }

    fn collect_emojis(node: &Node, emojis: &mut HashMap<&'static str, usize>) {
        if !node.is_dir {
            if let Some(ext) = node.path.extension().and_then(|s| s.to_str()) {
                if let Some(e) = ext_to_emoji(&ext.to_lowercase()) {
                    *emojis.entry(e).or_insert(0) += 1;
                }
            }
        } else {
            for child in &node.children {
                Self::collect_emojis(child, emojis);
            }
        }
    }
}

fn ext_to_emoji(ext: &str) -> Option<&'static str> {
    match ext {
        "rs" => Some("🦀"),
        "py" => Some("🐍"),
        "js" | "ts" | "jsx" | "tsx" => Some("📜"),
        "json" | "toml" | "yaml" | "yml" => Some("⚙️"),
        "md" | "txt" => Some("📝"),
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" => Some("🖼️"),
        "sh" | "bash" | "zsh" | "fish" => Some("🐚"),
        "html" | "css" => Some("🌐"),
        "c" | "cpp" | "h" | "hpp" => Some("🗜️"),
        "go" => Some("🐹"),
        "java" | "jar" => Some("☕"),
        _ => None,
    }
}

fn emoji_to_def(emoji: &str) -> &'static str {
    match emoji {
        "🦀" => "Rust",
        "🐍" => "Python",
        "📜" => "JavaScript/TypeScript",
        "⚙️" => "Config/Data",
        "📝" => "Text/Markdown",
        "🖼️" => "Image",
        "🐚" => "Shell script",
        "🌐" => "Web",
        "🗜️" => "C/C++",
        "🐹" => "Go",
        "☕" => "Java",
        _ => "File",
    }
}

fn get_modified(path: &Path) -> u64 {
    fs::symlink_metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn format_date(timestamp: u64) -> String {
    if timestamp == 0 {
        return "unknown".into();
    }
    let dt = Local.timestamp_opt(timestamp as i64, 0).unwrap();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn scan(
    path: &Path,
    current_depth: usize,
    sort_by: &SortBy,
    scanned_bytes: &Arc<AtomicU64>,
    gitignore: &ignore::gitignore::Gitignore,
    no_ignore: bool,
) -> std::io::Result<Node> {
    let metadata = fs::symlink_metadata(path)?;
    let is_dir = metadata.is_dir();
    let mut size = metadata.len();
    let mut file_count = if is_dir { 0 } else { 1 };
    let mut children = Vec::new();

    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if !is_dir {
        scanned_bytes.fetch_add(size, Ordering::Relaxed);
    }

    if is_dir {
        if let Ok(entries) = fs::read_dir(path) {
            let entries_vec: Vec<_> = entries.flatten().collect();
            let parsed_children: Vec<Node> = entries_vec.into_par_iter()
                .filter_map(|entry| {
                    let child_path = entry.path();
                    let name = child_path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                    if name == ".toon" {
                        return None;
                    }
                    let is_child_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    if !no_ignore {
                        if name == ".git" || name == "target" || name == "node_modules" {
                            return None;
                        }
                        if gitignore.matched(&child_path, is_child_dir).is_ignore() {
                            return None;
                        }
                    }
                    scan(&child_path, current_depth + 1, sort_by, scanned_bytes, gitignore, no_ignore).ok()
                })
                .collect();

            for child_node in &parsed_children {
                size += child_node.size;
                file_count += child_node.file_count;
            }
            children = parsed_children;
        }
    }

    match sort_by {
        SortBy::Size => children.sort_by(|a, b| b.size.cmp(&a.size)),
        SortBy::Name => children.sort_by(|a, b| a.name().cmp(&b.name())),
    }

    Ok(Node {
        path: path.to_path_buf(),
        size,
        file_count,
        is_dir,
        children,
        depth: current_depth,
        modified,
    })
}

fn get_color_for_depth(depth: usize) -> Color {
    match depth % 6 {
        0 => Color::Blue,
        1 => Color::Cyan,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Magenta,
        _ => Color::Red,
    }
}

fn print_recursive(
    n: &Node,
    prefix: &str,
    max_d: usize,
    root_size: u64,
    term_w: usize,
    limit: usize,
    stale_paths: &HashSet<PathBuf>,
    frame: usize,
    eta_sec: f64,
    lines: &mut u16,
    out: &mut impl Write,
) {
    if n.depth > max_d {
        return;
    }

    let visible_children: Vec<&Node> = n.children.iter().filter(|c| c.depth <= max_d).collect();

    let count = visible_children.len();
    let take_count = if limit > 0 {
        std::cmp::min(limit, count)
    } else {
        count
    };
    let final_children = &visible_children[0..take_count];

    let max_name_len = final_children
        .iter()
        .map(|c| {
            let name_with_emoji = format!("{} {}", c.emoji(), c.name());
            UnicodeWidthStr::width(name_with_emoji.as_str())
        })
        .max()
        .unwrap_or(0);

    for (i, child) in final_children.iter().enumerate() {
        let is_last = i == take_count - 1;
        let connector = if is_last { "└─ " } else { "├─ " };
        let name_with_emoji = format!("{} {}", child.emoji(), child.name());
        
        let is_stale = stale_paths.contains(&child.path);
        
        let size_str = if is_stale {
            let spinners = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let spinner = spinners[frame % spinners.len()];
            if eta_sec > 0.0 {
                format!("{} updating... ETA: {:.1}s", spinner, eta_sec).yellow().to_string()
            } else {
                format!("{} updating...", spinner).yellow().to_string()
            }
        } else {
            let s = format_size(child.size, DECIMAL);
            format!("{} (log: {})", s, format_date(child.modified))
        };

        let count_str = if child.is_dir {
            if is_stale {
                "".to_string()
            } else {
                format!(" ({})", child.file_count)
            }
        } else {
            String::new()
        };

        let visual_prefix_len = UnicodeWidthStr::width(prefix) + UnicodeWidthStr::width(connector);
        let name_len = UnicodeWidthStr::width(name_with_emoji.as_str());
        let padding = if max_name_len > name_len { max_name_len - name_len } else { 0 };
        
        // estimate visible length for bar
        let used_len = visual_prefix_len + max_name_len + 2 + UnicodeWidthStr::width(size_str.as_str()) + count_str.len() + 1;
        let bar_max_len = if term_w > used_len { term_w - used_len } else { 0 };

        let fraction = if root_size > 0 && !is_stale {
            child.size as f64 / root_size as f64
        } else {
            0.0
        };

        let bar_len = (bar_max_len as f64 * fraction).round() as usize;
        let bar = "█".repeat(bar_len);
        let color = get_color_for_depth(child.depth);
        let bar_color = if fraction > 0.5 { Color::Red } else if fraction > 0.2 { Color::Yellow } else { color };

        writeln!(
            out,
            "{}{}{}{}{}  {} {}{}",
            prefix,
            connector,
            name_with_emoji.color(color),
            if child.is_dir { "/" } else { "" }.color(color),
            " ".repeat(padding),
            bar.color(bar_color),
            size_str.white().dimmed(),
            count_str.white().dimmed()
        ).unwrap();

        *lines += 1;

        if child.is_dir {
            let next_prefix_char = if is_last { "   " } else { "│  " };
            for summary in child.emoji_summaries() {
                writeln!(out, "{}{}  {}", prefix, next_prefix_char, summary.dimmed()).unwrap();
                *lines += 1;
            }
        }

        let next_prefix_char = if is_last { "   " } else { "│  " };
        let next_prefix = format!("{}{}", prefix, next_prefix_char);
        print_recursive(
            child,
            &next_prefix,
            max_d,
            root_size,
            term_w,
            limit,
            stale_paths,
            frame,
            eta_sec,
            lines,
            out,
        );
    }
}

fn save_toon(root: &Node, path: &Path) {
    let toon_path = path.join(".toon");
    if let Ok(file) = File::create(&toon_path) {
        let _ = serde_json::to_writer(file, root);
    }
}

fn load_toon(path: &Path) -> Option<Node> {
    let toon_path = path.join(".toon");
    if let Ok(file) = File::open(&toon_path) {
        serde_json::from_reader(file).ok()
    } else {
        None
    }
}

fn collect_stale(node: &Node, max_depth: usize, stale_paths: &mut HashSet<PathBuf>) {
    if node.depth > max_depth {
        return;
    }
    
    let current_mod = get_modified(&node.path);
    if current_mod == 0 || current_mod > node.modified {
        stale_paths.insert(node.path.clone());
    }

    for child in &node.children {
        collect_stale(child, max_depth, stale_paths);
    }
}

fn get_bart_dir() -> PathBuf {
    let mut path = PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    path.push(".bart");
    path
}

fn get_indices_path() -> PathBuf {
    get_bart_dir().join("indices.json")
}

fn load_indices() -> HashSet<PathBuf> {
    let path = get_indices_path();
    if let Ok(content) = fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashSet::new()
    }
}

fn save_indices(indices: &HashSet<PathBuf>) {
    let dir = get_bart_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).unwrap();
    }
    let path = get_indices_path();
    if let Ok(file) = File::create(&path) {
        let _ = serde_json::to_writer_pretty(file, indices);
    }
}

fn manage_index(action: IndexCommands) {
    let mut indices = load_indices();
    match action {
        IndexCommands::Add { path } => {
            let abs_path = path.canonicalize().unwrap_or(path);
            if indices.insert(abs_path.clone()) {
                save_indices(&indices);
                println!("{} Added {} to the watch list.", "\u{2705}".green(), abs_path.display());
            } else {
                println!("{} {} is already in the watch list.", "\u{2139}\u{FE0F}".yellow(), abs_path.display());
            }
        }
        IndexCommands::Remove { path } => {
            let abs_path = path.canonicalize().unwrap_or(path);
            if indices.remove(&abs_path) {
                save_indices(&indices);
                println!("{} Removed {} from the watch list.", "\u{2705}".green(), abs_path.display());
            } else {
                println!("{} {} was not in the watch list.", "\u{2139}\u{FE0F}".yellow(), abs_path.display());
            }
        }
        IndexCommands::List => {
            println!("Tracked Directories:");
            if indices.is_empty() {
                println!("  (none)");
            } else {
                for p in &indices {
                    println!("  - {}", p.display());
                }
            }
        }
    }
}

fn get_discoveries_path() -> PathBuf {
    get_bart_dir().join("discoveries.json")
}

fn save_discoveries(discoveries: &HashMap<PathBuf, u64>) {
    let path = get_discoveries_path();
    if let Ok(file) = File::create(&path) {
        let _ = serde_json::to_writer_pretty(file, discoveries);
    }
}

fn load_discoveries() -> HashMap<PathBuf, u64> {
    let path = get_discoveries_path();
    if let Ok(content) = fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn get_pid_path() -> PathBuf {
    get_bart_dir().join("daemon.pid")
}

fn manage_daemon(action: DaemonCommands) {
    match action {
        DaemonCommands::Start => {
            let pid_path = get_pid_path();
            let pid = std::process::id();
            if let Ok(mut file) = File::create(&pid_path) {
                let _ = writeln!(file, "{}", pid);
            }
            
            println!("{} Inner Daemon started. Monitoring indexed paths...", "\u{2705}".green());
            
            let indices = load_indices();
            if indices.is_empty() {
                println!("No paths indexed. Run `bart index add <path>` first.");
                let _ = fs::remove_file(&pid_path);
                return;
            }

            // Start Outer Daemon thread for discovery
            let indices_clone = indices.clone();
            std::thread::spawn(move || {
                loop {
                    let mut discoveries = HashMap::new();
                    if let Ok(home) = std::env::var("HOME") {
                        let home_path = PathBuf::from(home);
                        if let Ok(entries) = fs::read_dir(&home_path) {
                            for entry in entries.flatten() {
                                let path = entry.path();
                                if path.is_dir() && !path.file_name().unwrap_or_default().to_string_lossy().starts_with('.') {
                                    let mut is_indexed = false;
                                    for idx in &indices_clone {
                                        if path.starts_with(idx) || idx.starts_with(&path) {
                                            is_indexed = true;
                                            break;
                                        }
                                    }
                                    if !is_indexed {
                                        let dummy = Arc::new(AtomicU64::new(0));
                                        let gitignore = GitignoreBuilder::new("").build().unwrap();
                                        if let Ok(node) = scan(&path, 0, &SortBy::Size, &dummy, &gitignore, true) {
                                            if node.size > 100_000_000 { // 100 MB threshold
                                                discoveries.insert(path, node.size);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    save_discoveries(&discoveries);
                    std::thread::sleep(Duration::from_secs(3600)); // sleep an hour
                }
            });

            let (tx, rx) = channel();
            let mut watcher = notify::recommended_watcher(tx).unwrap();

            for path in &indices {
                if path.exists() {
                    if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
                        eprintln!("Failed to watch {}: {:?}", path.display(), e);
                    } else {
                        println!("Watching {}", path.display());
                    }
                } else {
                    eprintln!("Warning: Indexed path {} does not exist.", path.display());
                }
            }

            for res in rx {
                match res {
                    Ok(event) => {
                        if let EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) = event.kind {
                            let mut affected_roots = HashSet::new();
                            for p in &event.paths {
                                let is_toon = p.file_name().and_then(|s| s.to_str()) == Some(".toon");
                                if !is_toon {
                                    for root in &indices {
                                        if p.starts_with(root) {
                                            affected_roots.insert(root.clone());
                                        }
                                    }
                                }
                            }
                            
                            for root in affected_roots {
                                let dummy_scanned = Arc::new(AtomicU64::new(0));
                                let mut builder = GitignoreBuilder::new(&root);
                                let _ = builder.add(root.join(".gitignore"));
                                let gitignore = builder.build().unwrap_or_else(|_| GitignoreBuilder::new("").build().unwrap());
                                
                                if let Ok(node) = scan(&root, 0, &SortBy::Size, &dummy_scanned, &gitignore, false) {
                                    save_toon(&node, &root);
                                }
                            }
                        }
                    },
                    Err(e) => eprintln!("watch error: {:?}", e),
                }
            }
            let _ = fs::remove_file(&pid_path);
        }
        DaemonCommands::Status => {
            let pid_path = get_pid_path();
            if pid_path.exists() {
                if let Ok(content) = fs::read_to_string(&pid_path) {
                    println!("Daemon Status: {} Running (PID: {})", "\u{2705}".green(), content.trim());
                } else {
                    println!("Daemon Status: \u{26A0}\u{FE0F} Running, but could not read PID");
                }
            } else {
                println!("Daemon Status: \u{26A0}\u{FE0F} Not running");
            }
            
            let discoveries = load_discoveries();
            if !discoveries.is_empty() {
                println!("\n\u{1F50D} Outer Daemon Discoveries:");
                let mut d_vec: Vec<_> = discoveries.into_iter().collect();
                d_vec.sort_by(|a, b| b.1.cmp(&a.1));
                for (p, size) in d_vec {
                    println!("  \u{26A0}\u{FE0F} Found massive unindexed directory: {} ({})", p.display().to_string().yellow(), format_size(size, DECIMAL).red());
                    println!("     Run `bart index add {}` to monitor it.", p.display());
                }
            }
        }
    }
}

fn main() {
    let args = Args::parse();

    if let Some(cmd) = args.command {
        match cmd {
            Commands::Index { action } => manage_index(action),
            Commands::Daemon { action } => manage_daemon(action),
        }
        return;
    }

    let path = &args.path;
    let path_clone = path.clone();
    let term_width = term_size::dimensions().map(|(w, _)| w).unwrap_or(80);

    let mut builder = GitignoreBuilder::new(&path_clone);
    let _ = builder.add(path_clone.join(".gitignore"));
    let gitignore = builder.build().unwrap_or_else(|_| GitignoreBuilder::new("").build().unwrap());

    let mut no_ignore = args.no_ignore;
    if args.clean || args.insights || args.explain {
        no_ignore = true;
    }

    if args.explain {
        let dummy_scanned = Arc::new(AtomicU64::new(0));
        let root = scan(path, 0, &args.sort, &dummy_scanned, &gitignore, no_ignore).unwrap();
        println!("Semantic size breakdown for '{}':", root.name().bold().blue());
        print_explain(&root);
        return;
    }

    if let Some(top_n) = args.top {
        let dummy_scanned = Arc::new(AtomicU64::new(0));
        let root = scan(path, 0, &args.sort, &dummy_scanned, &gitignore, no_ignore).unwrap();
        print_top(&root, top_n);
        return;
    }

    if args.clean {
        let dummy_scanned = Arc::new(AtomicU64::new(0));
        let root = scan(path, 0, &args.sort, &dummy_scanned, &gitignore, no_ignore).unwrap();
        run_clean(&root, args.apply);
        return;
    }

    if args.json || args.csv {
        let dummy_scanned = Arc::new(AtomicU64::new(0));
        let root = scan(path, 0, &args.sort, &dummy_scanned, &gitignore, no_ignore).unwrap();
        if args.json {
            println!("{}", serde_json::to_string_pretty(&root).unwrap());
        } else if args.csv {
            println!("Path,Size,FileCount,IsDir,Depth");
            fn print_csv(node: &Node) {
                println!("\"{}\",{},{},{},{}", node.path.display(), node.size, node.file_count, node.is_dir, node.depth);
                for child in &node.children {
                    print_csv(child);
                }
            }
            print_csv(&root);
        }
        return;
    }

    let cached_root = load_toon(path);
    
    if args.diff {
        if let Some(old_root) = cached_root {
            let dummy_scanned = Arc::new(AtomicU64::new(0));
            let new_root = scan(path, 0, &args.sort, &dummy_scanned, &gitignore, no_ignore).unwrap();
            println!("Differential scan against previous run:");
            print_diff(&old_root, &new_root, "", args.depth);
            save_toon(&new_root, path);
            return;
        } else {
            println!("No previous scan found for diff. Running normal scan first to create baseline.");
        }
    }

    let mut stale_paths = HashSet::new();

    if let Some(root) = &cached_root {
        collect_stale(root, args.depth, &mut stale_paths);
    }

    // If cache doesn't exist or is completely stale
    let needs_scan = cached_root.is_none() || !stale_paths.is_empty();

    if !needs_scan {
        let root = cached_root.unwrap();
        if args.filter {
            run_interactive_filter(&root, term_width, args.depth, args.limit);
        } else {
            let mut out = std::io::stdout();
            println!(
                "{} {} ({})",
                format!("{} {}", root.emoji(), root.name()).bold().blue(),
                format_size(root.size, DECIMAL).bold(),
                format!("{} files", root.file_count).white().dimmed()
            );
            for summary in root.emoji_summaries() {
                println!("  {}", summary.dimmed());
            }
            let mut lines = 0;
            print_recursive(
                &root,
                "",
                args.depth,
                root.size,
                term_width,
                args.limit,
                &stale_paths,
                0,
                0.0,
                &mut lines,
                &mut out,
            );
            if args.insights {
                print_insights(&root);
            }
        }
    } else {
        let root_to_render = if let Some(mut root) = cached_root {
            if let Ok(entries) = fs::read_dir(path) {
                let mut current_paths = HashSet::new();
                for entry in entries.flatten() {
                    let child_path = entry.path();
                    if child_path.file_name().and_then(|s| s.to_str()) == Some(".toon") {
                        continue;
                    }
                    current_paths.insert(child_path.clone());
                    
                    if !root.children.iter().any(|c| c.path == child_path) {
                        let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                        root.children.push(Node {
                            path: child_path.clone(),
                            size: 0,
                            file_count: if is_dir { 0 } else { 1 },
                            is_dir,
                            children: vec![],
                            depth: 1,
                            modified: 0,
                        });
                        stale_paths.insert(child_path);
                    }
                }
                root.children.retain(|c| current_paths.contains(&c.path));
            }
            root
        } else {
            let mut children = Vec::new();
            if let Ok(entries) = fs::read_dir(path) {
                for entry in entries.flatten() {
                    let child_path = entry.path();
                    if child_path.file_name().and_then(|s| s.to_str()) == Some(".toon") {
                        continue;
                    }
                    let is_dir = entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false);
                    children.push(Node {
                        path: child_path.clone(),
                        size: 0,
                        file_count: if is_dir { 0 } else { 1 },
                        is_dir,
                        children: vec![],
                        depth: 1,
                        modified: 0,
                    });
                    stale_paths.insert(child_path);
                }
            }
            Node {
                path: path.clone(),
                size: 0,
                file_count: 0,
                is_dir: true,
                children,
                depth: 0,
                modified: 0,
            }
        };
        
        if get_modified(path) > root_to_render.modified {
            stale_paths.insert(path.clone());
        }

        let scanned_bytes = Arc::new(AtomicU64::new(0));
        let scanned_bytes_clone = scanned_bytes.clone();
        
        let path_clone = path.clone();
        let sort_clone = args.sort.clone();
        
        let mut builder = GitignoreBuilder::new(&path_clone);
        let _ = builder.add(path_clone.join(".gitignore"));
        let gitignore = builder.build().unwrap_or_else(|_| GitignoreBuilder::new("").build().unwrap());
        
        let mut no_ignore = args.no_ignore;
        if args.clean || args.insights || args.explain {
            no_ignore = true;
        }

        let (tx, rx) = channel();
        
        std::thread::spawn(move || {
            let result = scan(&path_clone, 0, &sort_clone, &scanned_bytes_clone, &gitignore, no_ignore);
            let _ = tx.send(result);
        });

        let mut out = stdout();
        out.execute(cursor::Hide).unwrap();
        
        let start_time = Instant::now();
        let mut frame = 0;
        
        loop {
            if let Ok(res) = rx.try_recv() {
                out.execute(terminal::Clear(terminal::ClearType::FromCursorDown)).unwrap();
                match res {
                    Ok(new_root) => {
                        if args.filter {
                            run_interactive_filter(&new_root, term_width, args.depth, args.limit);
                        } else {
                            println!(
                                "{} {} ({})",
                                format!("{} {}", new_root.emoji(), new_root.name()).bold().blue(),
                                format_size(new_root.size, DECIMAL).bold(),
                                format!("{} files", new_root.file_count).white().dimmed()
                            );
                            for summary in new_root.emoji_summaries() {
                                println!("  {}", summary.dimmed());
                            }
                            let mut lines = 0;
                            stale_paths.clear(); // done scanning
                            print_recursive(
                                &new_root,
                                "",
                                args.depth,
                                new_root.size,
                                term_width,
                                args.limit,
                                &stale_paths,
                                0,
                                0.0,
                                &mut lines,
                                &mut out,
                            );
                            if args.insights {
                                print_insights(&new_root);
                            }
                        }
                        save_toon(&new_root, path);
                    }
                    Err(e) => {
                        eprintln!("Error scanning directory: {}", e);
                    }
                }
                break;
            }

            // Calculate ETA
            let elapsed = start_time.elapsed().as_secs_f64();
            let current = scanned_bytes.load(Ordering::Relaxed);
            let total = root_to_render.size.max(1); // avoid div zero
            
            let eta = if current > 0 && total > current {
                let speed = current as f64 / elapsed;
                let remaining = total.saturating_sub(current);
                remaining as f64 / speed
            } else {
                0.0
            };

            // draw frame
            let is_root_stale = stale_paths.contains(&root_to_render.path);
            let root_size_str = if is_root_stale {
                let spinners = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                let spinner = spinners[frame % spinners.len()];
                if eta > 0.0 {
                    format!("{} ETA: {:.1}s", spinner, eta).yellow().to_string()
                } else {
                    format!("{} updating...", spinner).yellow().to_string()
                }
            } else {
                format_size(root_to_render.size, DECIMAL)
            };

            println!(
                "{} {} ({})",
                format!("{} {}", root_to_render.emoji(), root_to_render.name()).bold().blue(),
                root_size_str.bold(),
                if is_root_stale { String::new() } else { format!("{} files", root_to_render.file_count).white().dimmed().to_string() }
            );
            for summary in root_to_render.emoji_summaries() {
                println!("  {}", summary.dimmed());
            }

            let mut lines_printed = 0;
            print_recursive(
                &root_to_render,
                "",
                args.depth,
                root_to_render.size,
                term_width,
                args.limit,
                &stale_paths,
                frame,
                eta,
                &mut lines_printed,
                &mut out,
            );
            
            out.flush().unwrap();
            
            // Wait and clear
            std::thread::sleep(Duration::from_millis(100));
            frame += 1;
            
            // Move cursor back up
            out.queue(cursor::MoveUp((lines_printed + 1) as u16)).unwrap();
            out.queue(terminal::Clear(terminal::ClearType::FromCursorDown)).unwrap();
        }
        
        out.execute(cursor::Show).unwrap();
    }

    if args.watch {
        let (tx, rx) = channel();
        let mut watcher = notify::recommended_watcher(tx).unwrap();
        watcher.watch(path, RecursiveMode::Recursive).unwrap();

        println!("Watching for changes...");
        for res in rx {
            match res {
                Ok(event) => {
                    if let EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) = event.kind {
                        let is_toon = event.paths.iter().any(|p| p.file_name().and_then(|s| s.to_str()) == Some(".toon"));
                        if !is_toon {
                            let dummy_scanned = Arc::new(AtomicU64::new(0));
                            let mut builder = GitignoreBuilder::new(path);
                            let _ = builder.add(path.join(".gitignore"));
                            let gitignore = builder.build().unwrap_or_else(|_| GitignoreBuilder::new("").build().unwrap());
                            if let Ok(root) = scan(path, 0, &args.sort, &dummy_scanned, &gitignore, args.no_ignore) {
                                save_toon(&root, path);
                            }
                        }
                    }
                },
                Err(e) => eprintln!("watch error: {:?}", e),
            }
        }
    }
}

use crossterm::event::{read, Event, KeyCode, KeyEvent};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};

fn get_editor() -> String {
    if let Some(home) = std::env::var_os("HOME") {
        let rc_path = PathBuf::from(home).join(".bartrc");
        if let Ok(content) = fs::read_to_string(&rc_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.starts_with("EDITOR=") {
                    return line["EDITOR=".len()..].trim().to_string();
                } else if !line.is_empty() && !line.starts_with('#') {
                    return line.to_string();
                }
            }
        }
    }
    std::env::var("EDITOR").unwrap_or_else(|_| "nano".to_string())
}

fn run_interactive_filter(root: &Node, term_w: usize, max_d: usize, _limit: usize) {
    let mut exts = HashSet::new();
    fn collect(node: &Node, exts: &mut HashSet<String>, max_d: usize) {
        if node.depth > max_d { return; }
        if !node.is_dir {
            if let Some(ext) = node.path.extension().and_then(|s| s.to_str()) {
                if ext_to_emoji(&ext.to_lowercase()).is_some() {
                    exts.insert(ext.to_lowercase());
                }
            }
        }
        for child in &node.children {
            collect(child, exts, max_d);
        }
    }
    collect(root, &mut exts, max_d);
    
    let mut formats: Vec<String> = exts.into_iter().collect();
    formats.sort();
    formats.insert(0, "All".to_string());
    formats.push("Directories".to_string());

    let mut format_idx = 0;
    let mut selected_idx = 0;
    
    enable_raw_mode().unwrap();
    let mut out = stdout();
    out.execute(cursor::Hide).unwrap();
    out.execute(terminal::EnterAlternateScreen).unwrap();
    
    loop {
        let current_filter = &formats[format_idx];
        let mut flat_nodes = Vec::new();
        
        fn flatten<'a>(node: &'a Node, filter: &str, flat: &mut Vec<(String, &'a Node)>, prefix: String, max_d: usize) {
            if node.depth > max_d { return; }
            
            let matches = if filter == "All" {
                true
            } else if filter == "Directories" {
                node.is_dir
            } else {
                !node.is_dir && node.path.extension().and_then(|s| s.to_str()).map(|s| s.to_lowercase()) == Some(filter.to_string())
            };
            
            if matches {
                flat.push((prefix.clone(), node));
            }
            
            let children: Vec<_> = node.children.iter().filter(|c| c.depth <= max_d).collect();
            for (i, child) in children.iter().enumerate() {
                let is_last = i == children.len() - 1;
                let next_prefix_char = if is_last { "   " } else { "│  " };
                flatten(child, filter, flat, format!("{}{}", prefix, next_prefix_char), max_d);
            }
        }
        
        flatten(root, current_filter, &mut flat_nodes, "".to_string(), max_d);
        
        if selected_idx >= flat_nodes.len() {
            selected_idx = flat_nodes.len().saturating_sub(1);
        }
        
        out.queue(Clear(ClearType::All)).unwrap();
        out.queue(cursor::MoveTo(0, 0)).unwrap();
        
        let mut header = String::new();
        for (i, f) in formats.iter().enumerate() {
            if i == format_idx {
                header.push_str(&format!("[{}] ", f.bold().blue()));
            } else {
                header.push_str(&format!("{} ", f.dimmed()));
            }
        }
        writeln!(out, "{}\r", header).unwrap();
        writeln!(out, "{}\r", "─".repeat(term_w)).unwrap();
        
        let term_h = term_size::dimensions().map(|(_, h)| h).unwrap_or(24);
        let list_h = term_h.saturating_sub(4);
        
        let start_idx = if selected_idx > list_h / 2 {
            selected_idx - list_h / 2
        } else {
            0
        };
        
        for i in start_idx..start_idx + list_h {
            if i >= flat_nodes.len() { break; }
            let (_, node) = &flat_nodes[i];
            
            let prefix = if i == selected_idx { "> " } else { "  " };
            let name_with_emoji = format!("{} {}", node.emoji(), node.name());
            
            let s = format_size(node.size, DECIMAL);
            let size_str = format!("{} (log: {})", s, format_date(node.modified));
            let count_str = if node.is_dir { format!(" ({})", node.file_count) } else { String::new() };
            
            let color = get_color_for_depth(node.depth);
            
            let line = format!(
                "{}{}{}{}  {} {}",
                prefix,
                name_with_emoji.color(color),
                if node.is_dir { "/" } else { "" }.color(color),
                if i == selected_idx { "  <--".yellow() } else { "".normal() },
                size_str.white().dimmed(),
                count_str.white().dimmed()
            );
            
            writeln!(out, "{}\r", line).unwrap();
        }
        
        out.flush().unwrap();
        
        if let Event::Key(KeyEvent { code, .. }) = read().unwrap() {
            match code {
                KeyCode::Esc => break,
                KeyCode::Left => {
                    if format_idx > 0 { format_idx -= 1; }
                }
                KeyCode::Right => {
                    if format_idx < formats.len() - 1 { format_idx += 1; }
                }
                KeyCode::Up => {
                    if selected_idx > 0 { selected_idx -= 1; }
                }
                KeyCode::Down => {
                    if selected_idx < flat_nodes.len().saturating_sub(1) { selected_idx += 1; }
                }
                KeyCode::Enter => {
                    if !flat_nodes.is_empty() {
                        let (_, selected_node) = &flat_nodes[selected_idx];
                        handle_action(selected_node);
                        enable_raw_mode().unwrap();
                        out.execute(terminal::EnterAlternateScreen).unwrap();
                    }
                }
                _ => {}
            }
        }
    }
    
    out.execute(terminal::LeaveAlternateScreen).unwrap();
    out.execute(cursor::Show).unwrap();
    disable_raw_mode().unwrap();
}

fn handle_action(node: &Node) {
    disable_raw_mode().unwrap();
    let mut out = stdout();
    out.execute(terminal::LeaveAlternateScreen).unwrap();
    out.execute(cursor::Show).unwrap();
    
    println!("Selected: {}", node.path.display());
    println!("1) Open (xdg-open)");
    println!("2) Edit ({})", get_editor());
    println!("3) Remove (rm)");
    println!("4) Cancel");
    print!("Choose an action: ");
    out.flush().unwrap();
    
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    
    match input.trim() {
        "1" => {
            let _ = std::process::Command::new("xdg-open").arg(&node.path).status();
        }
        "2" => {
            let editor = get_editor();
            let _ = std::process::Command::new(editor).arg(&node.path).status();
        }
        "3" => {
            if node.is_dir {
                let _ = std::fs::remove_dir_all(&node.path);
            } else {
                let _ = std::fs::remove_file(&node.path);
            }
        }
        _ => {}
    }
}

fn print_diff(old: &Node, new: &Node, prefix: &str, max_d: usize) {
    if new.depth > max_d { return; }
    
    let old_map: std::collections::HashMap<_, _> = old.children.iter().map(|c| (&c.path, c)).collect();
    let new_map: std::collections::HashMap<_, _> = new.children.iter().map(|c| (&c.path, c)).collect();
    
    let all_paths: std::collections::HashSet<_> = old_map.keys().chain(new_map.keys()).copied().collect();
    let mut paths: Vec<_> = all_paths.into_iter().collect();
    paths.sort();
    
    for (i, p) in paths.iter().enumerate() {
        let is_last = i == paths.len() - 1;
        let connector = if is_last { "└─ " } else { "├─ " };
        
        match (old_map.get(p), new_map.get(p)) {
            (Some(o), Some(n)) => {
                let size_diff = n.size as i64 - o.size as i64;
                if size_diff != 0 || o.file_count != n.file_count {
                    let diff_str = if size_diff > 0 {
                        format!("+{}", humansize::format_size(size_diff as u64, humansize::DECIMAL)).red()
                    } else if size_diff < 0 {
                        format!("-{}", humansize::format_size(size_diff.unsigned_abs(), humansize::DECIMAL)).green()
                    } else {
                        "".normal()
                    };
                    
                    println!("{}{} {} (Δ {})", prefix, connector, n.name(), diff_str);
                }
                let next_prefix = format!("{}{}", prefix, if is_last { "   " } else { "│  " });
                print_diff(o, n, &next_prefix, max_d);
            }
            (None, Some(n)) => {
                println!("{}{} {} (NEW: +{})", prefix, connector, n.name().green(), humansize::format_size(n.size, humansize::DECIMAL).green());
            }
            (Some(o), None) => {
                println!("{}{} {} (DEL: -{})", prefix, connector, o.name().red(), humansize::format_size(o.size, humansize::DECIMAL).red());
            }
            _ => {}
        }
    }
}

fn print_explain(root: &Node) {
    let mut categories: HashMap<String, u64> = HashMap::new();
    
    fn categorize(node: &Node, cats: &mut HashMap<String, u64>) {
        if !node.is_dir {
            let path_str = node.path.to_string_lossy().to_string();
            let cat = if path_str.contains("/target/debug/incremental") || path_str.contains("/target/release/incremental") || path_str.contains(r"\target\debug\incremental") || path_str.contains(r"\target\release\incremental") {
                "Rust Build: Incremental Cache"
            } else if path_str.contains("/target/debug/deps") || path_str.contains("/target/release/deps") || path_str.contains(r"\target\debug\deps") || path_str.contains(r"\target\release\deps") {
                "Rust Build: Dependencies (Deps)"
            } else if path_str.contains("/target/debug/build") || path_str.contains("/target/release/build") || path_str.contains(r"\target\debug\build") || path_str.contains(r"\target\release\build") {
                "Rust Build: Build Scripts"
            } else if path_str.contains("/target/debug") || path_str.contains("/target/release") || path_str.contains(r"\target\debug") || path_str.contains(r"\target\release") {
                "Rust Build: Artifacts/Binaries"
            } else if path_str.contains("/target/") || path_str.contains(r"\target\") {
                "Rust Build: Other Target Data"
            } else if path_str.contains("/node_modules/") || path_str.contains("/vendor/") || path_str.contains("/.cargo/registry") || path_str.contains(r"\node_modules\") || path_str.contains(r"\vendor\") {
                "Vendored Dependencies"
            } else if path_str.contains("/.git/") || path_str.contains(r"\.git\") {
                "Version Control (.git)"
            } else {
                if let Some(ext) = node.path.extension().and_then(|s| s.to_str()) {
                    let ext_lower = ext.to_lowercase();
                    match ext_lower.as_str() {
                        "rs" => "Rust Source Code",
                        "py" => "Python Source Code",
                        "js" | "ts" | "jsx" | "tsx" => "JS/TS Source Code",
                        "json" | "toml" | "yaml" | "yml" => "Configuration / Data",
                        "md" | "txt" => "Documentation / Text",
                        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "ico" => "Images / Media",
                        "sh" | "bash" | "zsh" | "fish" => "Shell Scripts",
                        "html" | "css" => "Web Assets",
                        "c" | "cpp" | "h" | "hpp" => "C/C++ Source Code",
                        "go" => "Go Source Code",
                        "java" | "jar" => "Java Code / Archives",
                        _ => "Other Files",
                    }
                } else {
                    "Other Files"
                }
            };
            *cats.entry(cat.to_string()).or_insert(0) += node.size;
        }
        for child in &node.children {
            categorize(child, cats);
        }
    }
    
    categorize(root, &mut categories);
    
    let mut sorted_cats: Vec<_> = categories.into_iter().collect();
    sorted_cats.sort_by(|a, b| b.1.cmp(&a.1));
    
    let total_size: u64 = sorted_cats.iter().map(|(_, size)| *size).sum();
    
    println!("Total Size Explained: {}", format_size(total_size, DECIMAL).bold());
    println!();
    for (name, size) in sorted_cats {
        let percentage = if total_size > 0 {
            (size as f64 / total_size as f64) * 100.0
        } else {
            0.0
        };
        println!(
            "{:>6.1}%  {:<12} {}",
            percentage,
            format_size(size, DECIMAL),
            name.cyan()
        );
    }
}

fn print_top(root: &Node, n: usize) {
    let mut files = Vec::new();
    fn collect(node: &Node, files: &mut Vec<(PathBuf, u64)>) {
        if !node.is_dir {
            files.push((node.path.clone(), node.size));
        }
        for child in &node.children {
            collect(child, files);
        }
    }
    collect(root, &mut files);
    files.sort_by(|a, b| b.1.cmp(&a.1));
    let take = files.into_iter().take(n).collect::<Vec<_>>();
    println!("Top {} largest files globally:", n);
    for (i, (path, size)) in take.iter().enumerate() {
        println!("{}. {} \u{2192} {}", i + 1, path.display().to_string().cyan(), format_size(*size, DECIMAL).yellow());
    }
}

fn print_insights(root: &Node) {
    let total_size = root.size;
    
    fn calc_bloat(node: &Node) -> u64 {
        let name = node.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if node.is_dir && (name == "target" || name == "node_modules" || name == ".git" || name == "vendor" || name == "__pycache__") {
            return node.size;
        } else if !node.is_dir && (name.ends_with(".log") || name == ".DS_Store") {
            return node.size;
        }
        
        let mut b = 0;
        for child in &node.children {
            b += calc_bloat(child);
        }
        b
    }
    let bloat_size = calc_bloat(root);
    
    if total_size > 0 {
        let pct = (bloat_size as f64 / total_size as f64) * 100.0;
        println!("\n\u{1F4A1} Auto-Insights:");
        if pct > 50.0 {
            println!("  \u{26A0}\u{FE0F}  {:.1}% of your project size is disposable bloat.", pct);
        } else {
            println!("  \u{2705} Your project looks reasonably clean ({:.1}% bloat).", pct);
        }
        
        let score = (pct / 10.0).min(10.0);
        println!("  Bloat Score: {:.1}/10", score);
        
        if bloat_size > 0 {
            println!("  \u{1F4A1} You may want to run `bart --clean` to see what can be safely ignored or removed.");
        }
    }

    let discoveries = load_discoveries();
    if !discoveries.is_empty() {
        println!("\n\u{1F50D} Outer Daemon Discoveries:");
        let mut d_vec: Vec<_> = discoveries.into_iter().collect();
        d_vec.sort_by(|a, b| b.1.cmp(&a.1));
        for (p, size) in d_vec {
            println!("  \u{26A0}\u{FE0F}  Found massive unindexed directory: {} ({})", p.display().to_string().yellow(), format_size(size, DECIMAL).red());
        }
        println!("  \u{1F4A1} Run `bart index add <path>` to bring them under persistent monitoring.");
    }

    let pid_path = get_pid_path();
    if !pid_path.exists() {
        println!("\n\u{1F4A1} The bart daemon is not currently running.");
        println!("  Run `bart daemon start` to enable historical tracking and predictive cleanup.");
    }
}

fn run_clean(root: &Node, apply: bool) {
    let mut targets = Vec::new();
    fn find_targets(node: &Node, targets: &mut Vec<(PathBuf, u64, bool)>) {
        let name = node.path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if node.is_dir && (name == "target" || name == "node_modules" || name == "vendor" || name == "__pycache__") {
            targets.push((node.path.clone(), node.size, true));
            return; // don't recurse into them, we delete the whole dir
        } else if !node.is_dir && (name.ends_with(".log") || name == ".DS_Store") {
            targets.push((node.path.clone(), node.size, false));
        } else {
            for child in &node.children {
                find_targets(child, targets);
            }
        }
    }
    find_targets(root, &mut targets);
    
    if targets.is_empty() {
        println!("No disposable heavyweights found for cleanup.");
        return;
    }
    
    if apply {
        println!("Cleaning up...");
        let mut freed = 0;
        for (p, size, is_dir) in targets {
            let res = if is_dir { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
            if res.is_ok() {
                freed += size;
                println!("  Deleted {}", p.display());
            } else {
                println!("  Failed to delete {}", p.display());
            }
        }
        println!("Cleanup complete! Freed {}", format_size(freed, DECIMAL).green());
    } else {
        println!("Safe cleanup suggestions (Dry Run):");
        let mut total_potential = 0;
        for (p, size, _) in targets {
            println!("  {} \u{2192} {}", p.display().to_string().red(), format_size(size, DECIMAL).yellow());
            total_potential += size;
        }
        println!("\nTotal space that can be freed: {}", format_size(total_potential, DECIMAL).bold().green());
        println!("Run with `--clean --apply` to delete these files/directories.");
    }
}