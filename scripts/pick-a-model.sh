#!/bin/sh
# Chooses a model from the hardware it can actually see, then serves it.
#
# This runs inside the bundled runtime container, NOT inside Endora — which is the whole
# point. Endora has no GPU access and cannot detect one (verified: `nvidia-smi` is absent
# inside it while a container granted `--gpus all` reports the card fine). Putting the
# choice here keeps Endora's code exactly as ADR 0055 requires: it talks to an
# OpenAI-compatible URL and knows nothing about what is behind it.
#
# The ladder is deliberately NOT "the biggest that fits". Measured on this project's own
# eval battery: a 14B did not beat the 7B, and every model tested scored 0/3 on following
# an explicit instruction about verification. Size buys capability, not obedience — so this
# picks the biggest that runs *comfortably*, and the model layer's adoption loop is what
# improves on it later, gated on the battery rather than on specifications.
set -eu

vram_mb=0
if command -v nvidia-smi >/dev/null 2>&1; then
    vram_mb=$(nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits 2>/dev/null \
        | head -1 | tr -d ' ' || echo 0)
fi
[ -n "$vram_mb" ] || vram_mb=0

# Room for the context window and the runtime's own overhead: a model that exactly fills
# the card spills into system memory and crawls, which is worse than a smaller one.
if [ "$vram_mb" -ge 10000 ]; then
    repo="Qwen/Qwen2.5-7B-Instruct-GGUF"; file="qwen2.5-7b-instruct-q4_k_m*.gguf"; layers=99
    saw="${vram_mb} MiB of GPU"
elif [ "$vram_mb" -ge 5000 ]; then
    repo="Qwen/Qwen2.5-3B-Instruct-GGUF"; file="qwen2.5-3b-instruct-q4_k_m*.gguf"; layers=99
    saw="${vram_mb} MiB of GPU"
else
    repo="Qwen/Qwen2.5-1.5B-Instruct-GGUF"; file="qwen2.5-1.5b-instruct-q4_k_m*.gguf"; layers=0
    saw="no usable GPU, so CPU"
fi

echo "bundled runtime: saw ${saw} — serving ${repo}"
echo "bundled runtime: this is a starting point. Endora's model layer can measure and adopt"
echo "bundled runtime: something better, and you can point ENDORA_MODEL_URL anywhere instead."

exec /app/llama-server \
    --hf-repo "$repo" --hf-file "$file" \
    --host 0.0.0.0 --port 8080 \
    --n-gpu-layers "$layers" \
    --ctx-size 8192 \
    --alias bundled
