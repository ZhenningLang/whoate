"""Synthetic inputs only; no real HOME, transcripts, processes, or model calls."""
import hashlib
import io
import json
import unittest
from agent_oracle import first_pair, fingerprint, title_metadata


class PairOnly(io.BytesIO):
    def __init__(self, data):
        super().__init__(data)
        self.reads = 0

    def readline(self, *args):
        self.reads += 1
        if self.reads > 2:
            raise AssertionError("READ_LATER_BODY")
        return super().readline(*args)


class OracleTests(unittest.TestCase):
    def test_tail_projects_title_metadata_without_using_message_content(self):
        stream = io.BytesIO(b'{"type":"ai-title","aiTitle":"Old"}\n'
                            b'{"type":"user","message":{"content":"NOT_A_TITLE"}}\n'
                            b'{"type":"custom-title","customTitle":"Latest"}\n')
        self.assertEqual(title_metadata(stream), "Latest")

    def test_first_pair_stops_before_later_body_and_returns_only_digests(self):
        records = [
            {"type": "user", "message": {"content": "<system-reminder>HIDDEN</system-reminder>Hello"}},
            {"type": "assistant", "message": {"content": [{"type": "text", "text": "World"}]}},
            {"type": "user", "message": {"content": "FORBIDDEN_LATER_BODY"}},
        ]
        stream = PairOnly(("\n".join(json.dumps(r) for r in records) + "\n").encode())
        result = first_pair(stream)
        self.assertEqual(result["first_prompt"], {"is_null": False, "chars": 5,
                         "sha256": hashlib.sha256(b"Hello").hexdigest()})
        self.assertEqual(result["first_reply"], {"is_null": False, "chars": 5,
                         "sha256": hashlib.sha256(b"World").hexdigest()})
        self.assertEqual(stream.reads, 2)
        self.assertNotIn("Hello", json.dumps(result))
        self.assertNotIn("World", json.dumps(result))

    def test_fingerprint_distinguishes_null_empty_and_unicode_character_count(self):
        self.assertEqual(fingerprint(None), {"is_null": True, "chars": None, "sha256": None})
        self.assertEqual(fingerprint("")["chars"], 0)
        self.assertEqual(fingerprint("😀")["chars"], 1)


if __name__ == "__main__":
    unittest.main()
