#!/bin/sh
set -eu

if ! command -v uv >/dev/null 2>&1; then
  echo "error: uv is required; install it from https://docs.astral.sh/uv/" >&2
  exit 1
fi

cache_root="${KOKORO_BOOK_CACHE_DIR:-${HOME}/Library/Caches/kokoro-book}"
runtime_dir="${cache_root}/qwen-runtime"
runtime_commit="49596ac8b69b9ed377db311a73df838795f38a3d"

if [ ! -x "${runtime_dir}/bin/python" ]; then
  uv venv --python 3.12 "${runtime_dir}"
fi
uv pip install --python "${runtime_dir}/bin/python" \
  "mlx-audio[tts] @ git+https://github.com/Blaizzy/mlx-audio.git@${runtime_commit}"
"${runtime_dir}/bin/python" -c \
  "from importlib.metadata import version; import mlx_audio, mlx; print('Qwen runtime ready:', version('mlx-audio'), version('mlx'))"

echo "Runtime: ${runtime_dir}/bin/python"
echo "The first --provider qwen run downloads the pinned 1.83 GB model."
