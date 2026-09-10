# Repo Awareness Implementation

This document describes the repo awareness feature that has been implemented in the LLM CLI.

## Overview

The CLI now automatically detects the project type and provides context-aware commands and responses. This makes the assistant more intelligent about the codebase it's working with.

## Features Implemented

### 1. Project Type Detection

The CLI automatically detects the following project types:

- **Rust** (detects `Cargo.toml`)
- **Node.js** (detects `package.json`)
- **Python** (detects `pyproject.toml` or `setup.py`)
- **Go** (detects `go.mod`)

Detection happens:
- On startup
- When using the CLI's `cd <directory>` command

### 2. Context-Aware Commands

#### `run tests` / `test`
Automatically uses the right test command based on project type:
- Rust: `cargo test`
- Node.js: `npm test`
- Python: `pytest`
- Go: `go test ./...`

#### `build` (NEW)
Builds the project using the appropriate build system:
- Rust: `cargo build`
- Node.js: `npm run build`
- Go: `go build`

#### `explain project` / `project info` (NEW)
Shows information about the current project:
- Project name
- Project type
- Root directory
- Source directories

### 3. LLM Context Enhancement

The LLM now receives project context in its prompts:
```
Current project: llm_cli (Rust)
```

This helps the assistant give more relevant answers. For example, if you ask "how do I run tests?", it knows you're in a Rust project and will suggest `cargo test`.

### 4. Diff-Based Workflow (Infrastructure)

Added support for proposing code changes as diffs:
- `ApplyDiff` workflow type added to `workflow.rs`
- Supports preview, accept, and reject flow
- Can be extended in the future for LLM-driven code suggestions

## Implementation Details

### New Module: `src/repo.rs`

Contains:
- `ProjectType` enum (Rust, Node, Python, Go, Unknown)
- `RepoInfo` struct with project metadata
- Detection logic that walks up directory tree
- Simple manifest parsing (Cargo.toml, package.json)
- Project-specific command builders

### Modified Files

1. **`src/session.rs`**
   - Added `repo_info: Option<RepoInfo>` field
   - Refreshes project metadata after the in-app `cd` command

2. **`src/handlers.rs`**
   - Added `get_session_repo_info()` to `IntentDispatcher` trait
   - Updated `handle_run_tests_intent()` to use project-specific commands
   - Added `handle_build_intent()` for building projects
   - Added `handle_explain_project_intent()` for project info

3. **`src/tools.rs`**
   - Added "build" tool with examples
   - Added "explain_project" tool with examples

4. **`src/workflow.rs`**
   - Added `ApplyDiff` workflow variant
   - Added handler for diff acceptance/rejection

5. **`src/app.rs`**
   - Implemented `get_session_repo_info()` method
   - Enhanced LLM prompts with project context

## Usage Examples

```bash
# The CLI automatically detects your project
$ ./llm_cli

# In a Rust project:
> run tests
cargo test output:
...

# In a Node.js project:
> run tests
npm test output:
...

# Get project info:
> explain project
Project: my-app
Type: Node.js (npm)
Root: /home/user/my-app
Source dirs: /home/user/my-app/src, /home/user/my-app/lib

# Build the project:
> build
cargo build output:
...
```

## Design Principles

- **Minimal overhead**: Simple parsing, no heavy dependencies
- **Thin implementation**: Straightforward code, easy to maintain
- **Graceful degradation**: Works even without project detection
- **Extensible**: Easy to add new project types

## Future Enhancements

The infrastructure is in place for:
- LLM-driven code fix proposals using the ApplyDiff workflow
- More sophisticated project structure analysis
- Integration with language servers for deeper code understanding
- Support for additional project types (Java, Ruby, etc.)
