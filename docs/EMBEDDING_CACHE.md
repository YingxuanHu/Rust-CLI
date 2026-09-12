# Embedding Cache Feature

## Overview

The LLM CLI now supports **persistent embedding caching** to significantly speed up startup time. Instead of recalculating embeddings for all tool examples every time you start the application, embeddings are computed once and saved to disk.

## How It Works

1. **First Run**: On the first startup (or when the cache is invalid), the CLI computes embeddings for all tool examples using Ollama's embedding model and saves them to a cache file.

2. **Subsequent Runs**: The CLI loads pre-computed embeddings from the cache file, avoiding regeneration. Semantic routing embeds the current input once and compares tool examples in memory.

3. **Cache Invalidation**: The cache is automatically invalidated if:
   - The embedding model changes
   - The Ollama endpoint changes
   - The tool examples or their tool assignments change
   - Vectors are empty, inconsistent in dimension, or otherwise invalid
   - The cache version is incompatible
   - The cache file is corrupted

## Configuration

### Default Cache Location

The cache is automatically stored locally in your project directory:
- **All platforms**: `.llm-cli/embeddings.toml` (relative to where you run the CLI)

This keeps your system clean and makes the cache portable with your project.

### Custom Cache Location

You can customize the cache location in your config file (`config.toml`):

```toml
# Custom embedding cache path
embedding_cache_path = "/path/to/your/embeddings.toml"

# Custom embedding model (must match what Ollama has)
embedding_model = "nomic-embed-text"

# Ollama daemon address or HTTPS API base (also used by chat and the classifier)
ollama_host = "127.0.0.1:11434"
```

Or via environment variables:

```bash
export LLM_CLI_EMBEDDING_CACHE_PATH="/path/to/embeddings.toml"
export LLM_CLI_EMBEDDING_MODEL="nomic-embed-text"
export LLM_CLI_OLLAMA_HOST="127.0.0.1:11434"
```

## Cache File Format

The cache is stored as a TOML file with the following structure:

```toml
model = "nomic-embed-text"
ollama_host = "127.0.0.1:11434"
version = 2

[examples]
"example phrase" = ["tool_name", [embedding_vector...]]
# ... more examples
```

## Cache Management

### Viewing Cache Size

```bash
ls -lh .llm-cli/embeddings.toml
```

The cache file is typically around 1-2 MB and is stored in the `.llm-cli/` directory within your project.

### Clearing the Cache

To force recomputation of embeddings:

```bash
rm .llm-cli/embeddings.toml
```

The next startup will automatically regenerate the cache. Do not remove the
whole `.llm-cli/` directory just to clear embeddings: it also holds your
configuration, input history, learned workflows, and frecency data.

### Changing Models

If you switch to a different embedding model, the cache will automatically be invalidated and regenerated with the new model.

## Performance Impact

The regression suite verifies that a warm semantic lookup makes exactly one
embedding request, for the user input. Previously, it requested the input plus
every tool example again despite having a disk cache. This is a verified
reduction in request count, not a measured wall-clock speedup. Startup and
routing latency depend on hardware, model state, and catalog size; the earlier
10–20x estimate has no reproducible benchmark in this repository.

## Troubleshooting

### Cache Load Failures

If the cache fails to load, you'll see a warning message:

```
Warning: Could not initialize embeddings: <error>. Falling back to direct chat.
```

The CLI will then attempt to recompute embeddings. Common causes:
- Corrupted cache file (solution: delete it)
- Model mismatch (solution: update config or delete cache)
- Permissions issue (solution: check file permissions)

### Missing Embedding Model

If Ollama doesn't have the embedding model:

```
Tip: Run 'ollama pull nomic-embed-text' to enable semantic matching.
```

Run the suggested command to download the model.
