"""用 hf-mirror 镜像验证 bge-small-zh 模型可下载并输出 512 维"""

import os

os.environ["HF_ENDPOINT"] = "https://hf-mirror.com"

from sentence_transformers import SentenceTransformer

print("loading model ...")
model = SentenceTransformer("BAAI/bge-small-zh-v1.5")
print("model loaded OK")
v = model.encode(["你好，机器人"], normalize_embeddings=True)
print("vector dim:", len(v[0]))
print("vector head:", v[0][:5])