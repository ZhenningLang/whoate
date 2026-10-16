"""Independent, memory-only first-message oracle. Never prints message content."""
import hashlib
import json


def title_metadata(stream):
    end = stream.seek(0, 2)
    stream.seek(max(0, end - 64 * 1024))
    title = None
    for line in stream.read(64 * 1024).splitlines():
        try:
            record = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            # A bounded tail may begin/end inside a record. It is not a title.
            continue
        if not isinstance(record, dict) or record.get("type") not in ("ai-title", "custom-title"):
            continue
        # No fallback to message.content: only authorized metadata is projected.
        value = record.get("aiTitle") or record.get("customTitle")
        if isinstance(value, str):
            title = value
    return title


def fingerprint(value):
    if value is None:
        return {"is_null": True, "chars": None, "sha256": None}
    if not isinstance(value, str):
        raise ValueError("ORACLE_EXPECTED_TEXT")
    return {"is_null": False, "chars": len(value),
            "sha256": hashlib.sha256(value.encode("utf-8")).hexdigest()}


def clean(value, limit):
    if isinstance(value, list):
        value = "\n".join(item["text"] for item in value
                          if item.get("type") == "text" and isinstance(item.get("text"), str))
    if not isinstance(value, str):
        raise RuntimeError("QA23_ORACLE_TEXT_FIELD_MISSING")
    # Independent implementation of the registered wrapper contract. Each
    # iteration consumes a complete tag or stops; unmatched '<' is scanned once.
    parts, hidden, cursor = [], [], 0
    while cursor < len(value):
        start = value.find("<", cursor)
        if start < 0:
            if not hidden:
                parts.append(value[cursor:])
            break
        if not hidden:
            parts.append(value[cursor:start])
        end = value.find(">", start + 1)
        if end < 0:
            if not hidden:
                parts.append(value[start:])
            break
        tag = value[start + 1:end]
        words = tag.lstrip("/").split()
        name = words[0].rstrip("/") if words else ""
        if name in ("system-reminder", "pasted_content"):
            if tag.startswith("/"):
                if hidden and hidden[-1] == name:
                    hidden.pop()
            elif not tag.endswith("/"):
                hidden.append(name)
        elif not hidden:
            parts.append(value[start:end + 1])
        cursor = end + 1
    return "".join(parts).strip()[:limit] or None


def first_pair(stream):
    # This is the existing source contract's 256 KiB prefix, not a new budget.
    remaining, found, seen = 256 * 1024, {}, set()
    while remaining > 0:
        line = stream.readline(remaining)
        if not line:
            break
        remaining -= len(line)
        try:
            record = json.loads(line)
        except (ValueError, UnicodeDecodeError):
            raise RuntimeError("QA23_ORACLE_INVALID_PREFIX_RECORD") from None
        if not isinstance(record, dict):
            raise RuntimeError("QA23_ORACLE_INVALID_PREFIX_RECORD")
        uuid = record.get("uuid")
        if isinstance(uuid, str):
            if uuid in seen:
                continue
            seen.add(uuid)
        if record.get("isSidechain") is True:
            continue
        kind = record.get("type")
        if kind in ("user", "assistant") and kind not in found:
            message = record.get("message")
            if not isinstance(message, dict):
                raise RuntimeError("QA23_ORACLE_TEXT_FIELD_MISSING")
            found[kind] = fingerprint(clean(message.get("content"), 200 if kind == "user" else 120))
        if len(found) == 2:
            return {"first_prompt": found["user"], "first_reply": found["assistant"]}
    raise RuntimeError("QA23_ORACLE_FIRST_PAIR_MISSING")
