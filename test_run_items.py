import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

import concurrent_write

BIN = Path(__file__).parent / "skills/workitem-pipeline/bin"
RUN_ITEMS = Path(__file__).parent / "skills/workitem-pipeline/watch/run-items"


class RunItemsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name, "ws")
        self.file = self.root / "T-1-a/workitem.md"
        self.file.parent.mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def test_blocking_an_item_keeps_a_concurrent_write(self):
        self.file.write_text("---\ntitle: t\nagent_status: ok\nhandler_attempts: 3\n---\nbody\n")
        # The first read decides, the second is the write's own read.
        env = {**concurrent_write.env(self.tmp.name, self.file, "pr_state: open", after=2),
               "WORKITEM_STAGE": "build", "WORKITEM_DIR_ROOT": str(self.root), "WORKITEM_MOVE": str(BIN / "workitem-move")}
        items = json.dumps([{"dir": "T-1-a", "agent_status": "ok"}])
        subprocess.run(["python3", str(RUN_ITEMS)], input=items, text=True, env=env, check=True)
        self.assertEqual(
            self.file.read_text(),
            "---\ntitle: t\nagent_status: blocked: max attempts reached in stage build\nhandler_attempts: 3\npr_state: open\n---\nbody\n",
        )
        self.assertEqual(sorted(p.name for p in self.file.parent.iterdir()), ["workitem.md"])


if __name__ == "__main__":
    unittest.main()
