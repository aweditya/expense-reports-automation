"""Headless-browser regression tests for the workbench HTML (Stage 24).

Loads a rendered workbench.html in Chromium via Playwright and asserts
the JS parses cleanly + the DOM has the elements the FA's flows rely
on. Catches the class of regression where a template edit silently
breaks the JS (e.g. an unescaped quote in an inline string) — which
the Python + Rust test suites can't catch, since they don't exercise
JavaScript.

DESIGN — file:// not http://:
  We load the workbench via `file:///path/to/workbench.html` rather
  than spawning a Flask subprocess. Endpoints (POST /edit, /undo,
  /delete-line, GET /undo-available) are covered by the audit script
  + the existing curl-based eyeballs. Playwright's job here is
  narrow: did the JS parse, do the IIFEs wire up, are the FA-visible
  DOM elements present?

  This means fetch() calls in the JS will fail (no Flask), but the
  page still loads + the IIFEs still run + we can assert the DOM
  is wired correctly.

PREREQUISITES:
  ./.venv/bin/pip install playwright
  ./.venv/bin/playwright install chromium

  ~150 MiB browser download. Tests skip themselves (instead of
  failing) when Playwright isn't installed so CI without the
  browser doesn't block PRs on this suite.

USES the .scratch/uploads/csv8a/workbench.html artifact produced by
scripts/stage_eyeball.sh csv8a. If it's missing, the tests skip
themselves with a clear message.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
WORKBENCH = REPO_ROOT / ".scratch" / "uploads" / "csv8a" / "workbench.html"

try:
    from playwright.sync_api import sync_playwright
    PLAYWRIGHT_AVAILABLE = True
except ImportError:
    PLAYWRIGHT_AVAILABLE = False


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed — run `pip install playwright && "
                     "playwright install chromium`")
@unittest.skipUnless(WORKBENCH.exists(),
                     f"{WORKBENCH} not staged — run scripts/stage_eyeball.sh csv8a first")
class TestWorkbenchBrowser(unittest.TestCase):
    """Each test launches Chromium, navigates to the workbench, runs
    its assertions, closes. ~3-5s per test (browser launch + parse).
    Slow-ish but bounded; total suite under 30s for the few tests we
    have."""

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def setUp(self):
        self.page = self._browser.new_page()
        # Capture console events so each test can assert on them.
        self._console_errors: list[str] = []
        self._page_errors: list[str] = []
        self.page.on("console", lambda msg: (
            self._console_errors.append(msg.text)
            if msg.type == "error" else None
        ))
        self.page.on("pageerror", lambda exc: self._page_errors.append(str(exc)))

    def tearDown(self):
        self.page.close()

    def _load(self):
        url = f"file://{WORKBENCH.resolve()}"
        # wait_until=domcontentloaded — we don't need image/font loads,
        # just the script tags to have parsed + IIFEs to have run.
        self.page.goto(url, wait_until="domcontentloaded")

    # ─── Test cases ────────────────────────────────────────────────────────

    def test_no_javascript_parse_errors(self):
        """Catches the regression class where a template edit (e.g.
        unescaped quote in an inline string) breaks the JS at parse
        time. A working page should have ZERO page errors."""
        self._load()
        self.assertEqual(self._page_errors, [],
                         f"page parsed with JS errors: {self._page_errors}")

    def test_field_cards_have_data_path_attributes(self):
        """Stage 7 click-to-edit relies on every editable field card
        carrying a data-path attribute. If a Rust render change drops
        the attribute, edits silently break."""
        self._load()
        cards = self.page.query_selector_all('[data-path]')
        self.assertGreater(len(cards), 50,
                           f"expected many editable field cards, got {len(cards)}")

    def test_csv_download_links_render_in_hero(self):
        """Stage 8a+8b: hero should have at least one CSV download link.
        Hidden when count=0; visible otherwise. csv8a has both."""
        self._load()
        links = self.page.query_selector_all('a.download-link[href$=".csv"]')
        self.assertGreater(len(links), 0, "expected at least one CSV download link")

    def test_delete_buttons_on_every_transaction_line(self):
        """Stage 23: × delete button on each line summary, data-line-idx
        attribute. Count should equal the number of transaction lines."""
        self._load()
        line_cards = self.page.query_selector_all('details.line-card')
        delete_buttons = self.page.query_selector_all('button.line-delete[data-line-idx]')
        self.assertEqual(len(line_cards), len(delete_buttons),
                         f"every line should have a delete button: "
                         f"{len(line_cards)} lines, {len(delete_buttons)} buttons")

    def test_undo_button_present_in_hero(self):
        """Stage 23: undo button rendered in hero. Hidden by default
        (setupUndoButton JS reveals it on /undo-available poll). For
        file:// load the fetch fails so it stays hidden — we just
        check the element is in the DOM."""
        self._load()
        btn = self.page.query_selector('#undo-button')
        self.assertIsNotNone(btn, "undo button missing from hero")

    def test_evidence_toggle_present(self):
        """Stage 6 evidence toggle. Sanity that the hero markup is
        complete."""
        self._load()
        toggle = self.page.query_selector('#evidence-toggle')
        self.assertIsNotNone(toggle, "evidence-toggle checkbox missing")

    def test_no_console_errors_after_load(self):
        """Filters out the expected fetch failures from setupUndoButton
        (file:// can't fetch /uploads/.../undo-available) and asserts
        no other console errors. If the JS throws elsewhere, this
        catches it."""
        self._load()
        # The undo-button JS catches its own fetch error and just leaves
        # the button hidden — no console.error. If we see any console
        # errors at all, they're unexpected.
        unexpected = [e for e in self._console_errors
                      if "undo-available" not in e]
        self.assertEqual(unexpected, [],
                         f"unexpected console errors: {unexpected}")


if __name__ == "__main__":
    unittest.main()
