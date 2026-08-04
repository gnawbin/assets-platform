"""文本切片器：将视频混合文本按时间窗口/字符数切分为带时间戳的 VideoChunk

设计参考：docs/知识库模块/视频全量转文本与向量化记忆设计方案.md 第 4 章
策略：
- 先按时间窗口切分（默认 30s，与 FRAME_INTERVAL_SEC 对齐），时间可回溯
- 切片超长（> CHUNK_MAX_CHARS）时按字符数二次切分
- 语音段按 Whisper 断句尽量在句末切开
"""

import re
from dataclasses import dataclass, asdict
from datetime import datetime, timezone

import config

# 时间戳行模式：[mm:ss-mm:ss 语音/画面] 文本
TIMESTAMP_LINE_RE = re.compile(
    r"^\[(\d{1,2}):(\d{2})-(\d{1,2}):(\d{2})\s+([^\]]+)\]\s*(.*)$"
)

# 中英文句末标点（用于语义边界切分）
SENTENCE_END = "。！？；.!?;"


@dataclass
class VideoChunk:
    """视频切片数据结构（对齐设计 4.2）"""

    video_id: str            # 视频唯一 ID（文件 SHA-256）
    chunk_index: int         # 切片序号
    start_sec: int           # 起始时间（秒）
    end_sec: int             # 结束时间（秒）
    type: str                # voice / ocr / mixed
    text: str                # 切片文本（含时间戳前缀）
    file_name: str           # 原视频文件名（溯源）
    created_at: str          # 创建时间（ISO8601）

    def to_dict(self) -> dict:
        return asdict(self)


def _parse_ts_to_sec(m: str, s: str) -> int:
    """mm:ss → 秒"""
    return int(m) * 60 + int(s)


def _split_sentences(text: str) -> list[str]:
    """按句末标点切分句子，保留标点"""
    sentences = []
    buf = ""
    for ch in text:
        buf += ch
        if ch in SENTENCE_END:
            sentences.append(buf.strip())
            buf = ""
    if buf.strip():
        sentences.append(buf.strip())
    return sentences


def _split_text_by_chars(text: str, max_chars: int) -> list[str]:
    """按字符数切分，尽量在句末切开"""
    if len(text) <= max_chars:
        return [text]

    parts = []
    sentences = _split_sentences(text)
    buf = ""
    for sent in sentences:
        if len(buf) + len(sent) <= max_chars:
            buf += sent
        else:
            if buf:
                parts.append(buf)
            # 单句超长则硬切
            if len(sent) > max_chars:
                while sent:
                    parts.append(sent[:max_chars])
                    sent = sent[max_chars:]
            else:
                buf = sent
    if buf:
        parts.append(buf)
    return [p for p in parts if p.strip()]


def chunk_video_text(
    video_id: str,
    file_name: str,
    segments: list[dict],
    window_sec: int = None,
    max_chars: int = None,
    duration_sec: float = 0,
) -> list[VideoChunk]:
    """将带时间戳的视频文本段切分为 VideoChunk 列表

    参数：
    - video_id: 视频唯一 ID（SHA-256）
    - file_name: 原视频文件名
    - segments: 带时间戳的文本段列表（来自 VideoParser metadata["segments"]）
                [{time, type(voice|ocr), text}]
    - window_sec: 时间窗口（秒），默认取 config.CHUNK_WINDOW_SEC
    - max_chars: 切片最大字符数，默认取 config.CHUNK_MAX_CHARS
    - duration_sec: 视频总时长（秒），用于最后一个窗口封口

    返回：按时间排序的 VideoChunk 列表（chunk_index 从 0 递增）
    """
    window_sec = window_sec or config.CHUNK_WINDOW_SEC
    max_chars = max_chars or config.CHUNK_MAX_CHARS

    if not segments:
        return []

    # 1. 按时间窗口分组
    windows: dict[int, list[dict]] = {}
    for seg in segments:
        t = float(seg.get("time", 0)) or 0
        win = int(t // window_sec) * window_sec
        windows.setdefault(win, []).append(seg)

    created_at = datetime.now(timezone.utc).isoformat()
    chunks: list[VideoChunk] = []

    for win in sorted(windows.keys()):
        segs_in_win = windows[win]
        # 2. 组内按时间排序
        segs_in_win.sort(key=lambda s: float(s.get("time", 0)) or 0)

        # 3. 判定类型：全部 ocr → ocr；全部 voice → voice；混合 → mixed
        types = {s.get("type", "voice") for s in segs_in_win}
        if types == {"ocr"}:
            chunk_type = "ocr"
        elif types == {"voice"}:
            chunk_type = "voice"
        else:
            chunk_type = "mixed"

        # 4. 组内文本（含时间戳前缀）
        window_texts = []
        for s in segs_in_win:
            t = float(s.get("time", 0)) or 0
            typ = s.get("type", "voice")
            text = (s.get("text") or "").strip()
            if not text:
                continue
            end_t = min(t + window_sec, duration_sec or t + window_sec)
            window_texts.append(
                f"[{_format_ts(int(t))}-{_format_ts(int(end_t))} {typ}] {text}"
            )
        combined = "\n".join(window_texts)

        # 5. 超长二次切分（按字符数，尽量在句末）
        text_parts = _split_text_by_chars(combined, max_chars)

        win_end = min(win + window_sec, int(duration_sec)) if duration_sec else win + window_sec
        for i, part in enumerate(text_parts):
            chunk = VideoChunk(
                video_id=video_id,
                chunk_index=0,  # 末尾统一编号
                start_sec=win,
                end_sec=win_end,
                type=chunk_type,
                text=part,
                file_name=file_name,
                created_at=created_at,
            )
            chunks.append(chunk)

    # 6. 统一编号
    for idx, chunk in enumerate(chunks):
        chunk.chunk_index = idx

    return chunks


def _format_ts(sec: int) -> str:
    """秒 → mm:ss"""
    sec = max(0, int(sec))
    return f"{sec // 60:02d}:{sec % 60:02d}"