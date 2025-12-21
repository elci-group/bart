use clap::Parser;
use colored::*;
use humansize::{format_size, DECIMAL};
use std::fs;
use std::path::{Path, PathBuf};
use term_size;
use unicode_width::UnicodeWidthStr;

#[derive(clap::ValueEnum, Clone, Debug, Default)]
enum SortBy {
    #[default]
    Size,
    Name,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory to scan
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Maximum depth to display
    #[arg(short, long, default_value_t = 1)]
    depth: usize,

    /// Number of top entries to show per directory (0 for all)
    #[arg(short = 'n', long, default_value_t = 0)]
    limit: usize,

    /// Sort by size or name
    #[arg(short, long, value_enum, default_value_t = SortBy::Size)]
    sort: SortBy,
}

#[derive(Debug)]
struct Node {
    path: PathBuf,
    size: u64,
    file_count: usize,
    is_dir: bool,
    children: Vec<Node>,
    depth: usize,
}

impl Node {
    fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .to_string()
    }
}

fn scan(path: &Path, current_depth: usize, sort_by: &SortBy) -> std::io::Result<Node> {
    let metadata = fs::symlink_metadata(path)?;
    let is_dir = metadata.is_dir();
    let mut size = metadata.len();
    let mut file_count = if is_dir { 0 } else { 1 };
    let mut children = Vec::new();

    if is_dir {
        match fs::read_dir(path) {
            Ok(entries) => {
                for entry in entries {
                    if let Ok(entry) = entry {
                        let child_path = entry.path();
                        // Recursive call
                        match scan(&child_path, current_depth + 1, sort_by) {
                            Ok(child_node) => {
                                size += child_node.size;
                                file_count += child_node.file_count;
                                children.push(child_node);
                            }
                            Err(_) => {
                                // Permission denied or other error, ignore
                            }
                        }
                    }
                }
            }
            Err(_) => {
                // Permission denied
            }
        }
    }

    // Sort children
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

fn print_recursive(n: &Node, prefix: &str, max_d: usize, root_size: u64, term_w: usize, limit: usize) {
    if n.depth > max_d { return; }
    
    // Filter children within depth limit
    let visible_children: Vec<&Node> = n.children.iter()
        .filter(|c| c.depth <= max_d)
        .collect();
        
    let count = visible_children.len();
    let take_count = if limit > 0 { std::cmp::min(limit, count) } else { count };
    let final_children = &visible_children[0..take_count];

    // Calculate max name length for alignment in this group
    let max_name_len = final_children.iter()
        .map(|c| UnicodeWidthStr::width(c.name().as_str()))
        .max()
        .unwrap_or(0);

    for (i, child) in final_children.iter().enumerate() {
        let is_last = i == take_count - 1;
        
        let connector = if is_last { "└─ " } else { "├─ " };
        
        let name = child.name();
        let size_str = format_size(child.size, DECIMAL);
        let count_str = if child.is_dir {
            format!(" ({})", child.file_count)
        } else {
            String::new()
        };
        
        // Layout: prefix + connector + name + padding + "  " + bar + " " + size + count
        let visual_prefix_len = UnicodeWidthStr::width(prefix) + UnicodeWidthStr::width(connector);
        let name_len = UnicodeWidthStr::width(name.as_str());
        let padding = max_name_len - name_len;
        let size_len = size_str.len();
        let count_len = count_str.len();
        
        let used_len = visual_prefix_len + max_name_len + 2 + size_len + count_len + 1; 
        let bar_max_len = if term_w > used_len { term_w - used_len } else { 0 };
        
        let fraction = if root_size > 0 {
           child.size as f64 / root_size as f64
        } else { 0.0 };
        
        let bar_len = (bar_max_len as f64 * fraction).round() as usize;
        let bar = "█".repeat(bar_len);
        
        let color = get_color_for_depth(child.depth);
        
        println!("{}{}{}{}{}  {} {}{}", 
           prefix, 
           connector, 
           name.color(color), 
           if child.is_dir { "/" } else { "" }.color(color),
           " ".repeat(padding),
           bar.color(color), 
           size_str.white().dimmed(),
           count_str.white().dimmed()
        );
        
        // Recurse
        let next_prefix_char = if is_last { "   " } else { "│  " };
        let next_prefix = format!("{}{}", prefix, next_prefix_char);
        print_recursive(child, &next_prefix, max_d, root_size, term_w, limit);
    }
}

fn main() {
    let args = Args::parse();
    
    let path = &args.path;
    let term_width = term_size::dimensions().map(|(w, _)| w).unwrap_or(80);

    match scan(path, 0, &args.sort) {
        Ok(root) => {
            println!("{} {} ({})", 
                root.name().bold().blue(), 
                format_size(root.size, DECIMAL).bold(),
                format!("{} files", root.file_count).white().dimmed()
            );
            print_recursive(&root, "", args.depth, root.size, term_width, args.limit);
        }
        Err(e) => {
            eprintln!("Error scanning directory: {}", e);
            std::process::exit(1);
        }
    }
}
