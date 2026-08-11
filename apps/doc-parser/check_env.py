"""环境检查：huggingface 缓存、网络可用性、modelscope"""

import os
import glob

print("=== HF cache ===")
hub = os.path.expanduser("~/.cache/huggingface/hub")
print("path:", hub, "exists:", os.path.exists(hub))
if os.path.exists(hub):
    for name in sorted(os.listdir(hub)):
        print(" -", name)

print("\n=== bge models anywhere ===")
for root in [
    os.path.expanduser("~/.cache/huggingface"),
    os.path.expanduser("~/.cache/torch"),
]:
    for p in glob.glob(os.path.join(root, "**", "*bge*"), recursive=True):
        print(" -", p)

print("\n=== HF env ===")
for k, v in os.environ.items():
    if "HF_" in k or "huggingface" in k.lower():
        print(f" {k}={v}")

print("\n=== hf-mirror reachable ===")
import urllib.request

for url in ["https://hf-mirror.com", "https://huggingface.co"]:
    try:
        req = urllib.request.Request(url, method="HEAD")
        resp = urllib.request.urlopen(req, timeout=5)
        print(f" {url} -> HTTP {resp.status}")
    except Exception as e:
        print(f" {url} -> FAIL: {type(e).__name__}: {e}")

print("\n=== modelscope ===")
try:
    import modelscope

    print(" modelscope", modelscope.__version__)
except ImportError as e:
    print(" modelscope not installed:", e)

print("\n=== sentence-transformers local models ===")
try:
    from sentence_transformers import SentenceTransformer

    for m in ["BAAI/bge-small-zh-v1.5", "paraphrase-multilingual-MiniLM-L12-v2"]:
        try:
            cached = SentenceTransformer(m)
            print(f" {m} -> loaded OK, dim={cached.get_sentence_embedding_dimension()}")
        except Exception as e:
            print(f" {m} -> FAIL: {type(e).__name__}: {str(e)[:200]}")
except Exception as e:
    print(" import err:", e)