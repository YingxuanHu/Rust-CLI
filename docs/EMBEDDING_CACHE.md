# Embedding Cache Feature

## Overview

The LLM CLI now supports **persistent embedding caching** to significantly speed up startup time. Instead of recalculating embeddings for all tool examples every time you start the application, embeddings are computed once and saved to disk.

## How It Works

1. **First Run**: On the first startup (or when the cache is invalid), the CLI computes embeddings for all tool examples using Ollama's embedding model and saves them to a cache file.

2. **Subsequent Runs**: The CLI loads pre-computed embeddings from the cache file, making startup almost instantaneous.

3. **Cache Invalidation**: The cache is automatically invalidated if:
   - The embedding model changes
   - The cache version is incompatible
   - The cache file is corrupted

## Configuration

### Default Cache Location

The cache is automatically stored locally in your project directory:
- **All platforms**: `.cache/embeddings.toml` (relative to where you run the CLI)

This keeps your system clean and makes the cache portable with your project.

### Custom Cache Location

You can customize the cache location in your config file (`config.toml`):

```toml
# Custom embedding cache path
embedding_cache_path = "/path/to/your/embeddings.toml"

# Custom embedding model (must match what Ollama has)
embedding_model = "nomic-embed-text"
```

Or via environment variables:

```bash
export LLM_CLI_EMBEDDING_CACHE_PATH="/path/to/embeddings.toml"
export LLM_CLI_EMBEDDING_MODEL="nomic-embed-text"
```

## Cache File Format

The cache is stored as a TOML file with the following structure:

```toml
model = "nomic-embed-text"
version = 1

[examples]
"example phrase" = ["tool_name", [embedding_vector...]]
# ... more examples
```

## Cache Management

### Viewing Cache Size

```bash
ls -lh .cache/embeddings.toml
```

The cache file is typically around 1-2 MB and is stored in the `.cache/` directory within your project.

### Clearing the Cache

To force recomputation of embeddings:

```bash
rm .cache/embeddings.toml
# Or remove the entire cache directory
rm -rf .cache/
```

The next startup will automatically regenerate the cache.

### Changing Models

If you switch to a different embedding model, the cache will automatically be invalidated and regenerated with the new model.

## Performance Impact

- **Without Cache**: ~1-2 seconds to compute all embeddings on startup
- **With Cache**: ~0.1 seconds to load embeddings from disk

This results in a **10-20x speedup** for subsequent runs!

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

