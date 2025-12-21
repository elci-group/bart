<p align="center">
  <img src="bart-logo.png" alt="Bart Logo" width="500">
</p>

# Bart

Bart is a command-line tool written in Rust for analyzing and visualizing directory usage. It combines the hierarchical view of `tree` with the disk usage information of `du`, presenting it in a colorful, easy-to-read format.

## Features

- **Recursive Scanning**: Explore directory structures to any depth.
- **Disk Usage Visualization**: See file and directory sizes with visual bars.
- **Sorting**: Sort output by file size or name.
- **Filtering**: Limit the depth of display and the number of entries shown per directory.
- **Colorful Output**: Visual cues for different directory depths.

## Installation

Ensure you have Rust and Cargo installed.

```bash
cargo install --path .
```

## Usage

```bash
bart [OPTIONS] [PATH]
```

### Options

- `-d, --depth <DEPTH>`  
  Maximum depth to display (default: 1).

- `-n, --limit <LIMIT>`  
  Number of top entries to show per directory (0 for all).

- `-s, --sort <SORT>`  
  Sort by `size` or `name` (default: size).

- `-h, --help`  
  Print help.

- `-V, --version`  
  Print version.

### Examples

**Scan current directory (depth 1):**
```bash
bart
```

**Scan specific path with depth 3:**
```bash
bart /path/to/dir --depth 3
```

**Show top 5 largest files/folders:**
```bash
bart -n 5
```

**Sort by name instead of size:**
```bash
bart --sort name
```
