"""Headless-browser regression tests for the FA-facing UI.

Test classes:
  TestWorkbenchBrowser        static workbench load (file://)
  TestWorkbenchBrowserB1      split-extractor field cards
  TestWorkbenchBrowserB1Flows end-to-end workbench interactions
  TestWorkbenchBrowserB2      mileage card + IRS-rate computation
  TestUploadFormBrowser       form fields, localStorage, refresh

Catches JS-only regressions (broken inline strings, parser errors,
DOM elements the FA flows depend on) that Python and Rust suites
miss. Tests skip when Playwright or the relevant fixture is absent.

Setup:
  ./.venv/bin/pip install playwright
  ./.venv/bin/playwright install chromium
"""

from __future__ import annotations

import json
import subprocess
import sys
import unittest
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
WORKBENCH = REPO_ROOT / ".scratch" / "uploads" / "csv8a" / "workbench.html"

# Live-Flask fixtures. b1-eye exercises the split-call extractors;
# b2-eye exercises the mileage extractor via a synthetic per-doc JSON.
# Both are staged manually for the live tests; absent fixtures cause
# the relevant tests to skip.
B1_FIXTURE = REPO_ROOT / ".scratch" / "uploads" / "b1-eye"
B2_FIXTURE = REPO_ROOT / ".scratch" / "uploads" / "b2-eye"
B1_FLASK_URL = "http://localhost:8088"

# Import the localStorage key constant from the app so a Python-side
# rename can't silently leave the test asserting against the old key.
sys.path.insert(0, str(REPO_ROOT / "scripts"))
from local_app_simple import FORM_DRAFT_KEY  # noqa: E402

try:
    from playwright.sync_api import sync_playwright
    PLAYWRIGHT_AVAILABLE = True
except ImportError:
    PLAYWRIGHT_AVAILABLE = False


def _flask_reachable(url: str) -> bool:
    """Quick GET to check Flask is running before we try to load workbench
    over HTTP. Avoids cryptic Playwright timeouts when the user forgot
    to start Flask."""
    try:
        with urllib.request.urlopen(url + "/", timeout=2) as resp:
            return resp.status == 200
    except (urllib.error.URLError, ConnectionRefusedError, TimeoutError):
        return False


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed — run `pip install playwright && "
                     "playwright install chromium`")
@unittest.skipUnless(WORKBENCH.exists(),
                     f"{WORKBENCH} not staged — run scripts/stage_eyeball.sh csv8a first")
class TestWorkbenchBrowser(unittest.TestCase):
    """Static workbench load via file://. ~3-5s per test."""

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
        self.page.goto(f"file://{WORKBENCH.resolve()}", wait_until="domcontentloaded")

    def test_no_javascript_parse_errors(self):
        """A template edit that breaks JS at parse time would surface here."""
        self._load()
        self.assertEqual(self._page_errors, [],
                         f"page parsed with JS errors: {self._page_errors}")

    def test_field_cards_have_data_path_attributes(self):
        """Every editable card carries data-path; click-to-edit depends on it."""
        self._load()
        cards = self.page.query_selector_all('[data-path]')
        self.assertGreater(len(cards), 50,
                           f"expected many editable field cards, got {len(cards)}")

    def test_csv_download_links_render_in_hero(self):
        """Hero has at least one CSV download link when CSVs are non-empty."""
        self._load()
        links = self.page.query_selector_all('a.download-link[href$=".csv"]')
        self.assertGreater(len(links), 0, "expected at least one CSV download link")

    def test_delete_buttons_on_every_transaction_line(self):
        """Each transaction line has a delete button."""
        self._load()
        line_cards = self.page.query_selector_all('details.line-card')
        delete_buttons = self.page.query_selector_all('button.line-delete[data-line-idx]')
        self.assertEqual(len(line_cards), len(delete_buttons),
                         f"every line should have a delete button: "
                         f"{len(line_cards)} lines, {len(delete_buttons)} buttons")

    def test_undo_button_present_in_hero(self):
        """Undo button exists in DOM (hidden until /undo-available reveals it)."""
        self._load()
        self.assertIsNotNone(self.page.query_selector('#undo-button'),
                             "undo button missing from hero")

    def test_evidence_toggle_present(self):
        """Evidence toggle checkbox exists in the hero."""
        self._load()
        self.assertIsNotNone(self.page.query_selector('#evidence-toggle'),
                             "evidence-toggle checkbox missing")

    def test_no_console_errors_after_load(self):
        """No unexpected console errors. Filters the known file:// fetch
        failure for /undo-available, which the page handles silently."""
        self._load()
        unexpected = [e for e in self._console_errors if "undo-available" not in e]
        self.assertEqual(unexpected, [],
                         f"unexpected console errors: {unexpected}")


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(B1_FIXTURE.exists(),
                     f"{B1_FIXTURE} not staged")
@unittest.skipUnless(_flask_reachable(B1_FLASK_URL),
                     f"Flask not reachable at {B1_FLASK_URL} — start it with "
                     "`PORT=8088 VERTEX_PROJECT_ID=soe-agile-agents "
                     "./.venv/bin/python scripts/local_app_simple.py &`")
class TestWorkbenchBrowserB1(unittest.TestCase):
    """Live-Flask render of the multi-call meal + transport extractor
    output. Asserts the per-receipt amount fields (pre_tax_amount,
    tax_amount, tip_amount) render with correct labels and are
    click-to-editable. Loads over HTTP so /edit fetch round-trips."""

    URL = f"{B1_FLASK_URL}/uploads/b1-eye/workbench.html"
    EDIT_URL = f"{B1_FLASK_URL}/uploads/b1-eye/edit"
    REPORT_PATH = B1_FIXTURE / "reduced" / "report.json"

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def setUp(self):
        self.page = self._browser.new_page(viewport={"width": 1400, "height": 1800})
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
        self.page.goto(self.URL, wait_until="networkidle")

    def test_b1_screenshot_for_eyeball(self):
        """Save a full-page screenshot for visual eyeball."""
        self._load()
        out = REPO_ROOT / ".scratch" / "b1-eye" / "workbench-screenshot.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        self.page.screenshot(path=str(out), full_page=True)
        self.assertGreater(out.stat().st_size, 10_000,
                           "screenshot file suspiciously small")

    def test_b1_meal_card_labels_render(self):
        """All meal-detail labels show up in the rendered HTML."""
        self._load()
        body = self.page.content()
        for label in ["Venue", "Subtotal (pre-tax)", "Tax", "Tip",
                      "Alcohol", "Has Alcohol"]:
            self.assertIn(f">{label}<", body,
                          f"meal card label '{label}' missing")

    def test_b1_transport_card_labels_render(self):
        """All transport-detail labels show up. `&` HTML-escapes to `&amp;`."""
        self._load()
        body = self.page.content()
        for label in ["Service Provider", "Origin", "Destination",
                      "Fare (pre-tax)", "Taxes &amp; Fees", "Driver Tip"]:
            self.assertIn(f">{label}<", body,
                          f"transport card label '{label}' missing")

    def test_b1_new_field_cards_have_data_path_attrs(self):
        """Every editable detail card has a data-path; click-to-edit needs it."""
        self._load()
        for path_suffix in [
            "meal_details.pre_tax_amount",
            "meal_details.tax_amount",
            "ground_transport_details.pre_tax_amount",
            "ground_transport_details.tax_amount",
            "ground_transport_details.tip_amount",
        ]:
            els = self.page.query_selector_all(f'[data-path$="{path_suffix}"]')
            self.assertGreater(len(els), 0,
                               f"no data-path element ending in '{path_suffix}'")

    def test_b1_click_pre_tax_opens_inline_editor(self):
        """Clicking the value in a pre_tax_amount card opens the inline editor."""
        self._load()
        card = self.page.query_selector(
            '[data-path$="meal_details.pre_tax_amount"]'
        )
        self.assertIsNotNone(card, "pre_tax_amount card not found")
        value_el = card.query_selector(".field-value")
        self.assertIsNotNone(value_el, "field-value not found inside card")
        value_el.click()
        input_el = card.query_selector("input.inline-edit-input")
        self.assertIsNotNone(input_el,
                             "click did not open inline editor on new field")

    def test_b1_edit_endpoint_round_trip(self):
        """POST a new pre_tax value to /edit; assert it persists on disk."""
        # Snapshot original value from the on-disk report so we can
        # restore at end and compute the new value.
        original_report = json.loads(self.REPORT_PATH.read_text())
        meal_idx = next(
            i for i, line in enumerate(
                original_report["transaction_lines"])
            if line.get("meal_details")
            and line["meal_details"]["pre_tax_amount"]["value"] is not None
        )
        path = f"expense_report.transaction_lines[{meal_idx}].meal_details.pre_tax_amount"
        original_value = original_report["transaction_lines"][meal_idx][
            "meal_details"]["pre_tax_amount"]["value"]
        new_value = round(original_value + 7.77, 2)

        try:
            req = urllib.request.Request(
                self.EDIT_URL,
                data=json.dumps({"path": path, "value": str(new_value)}).encode(),
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            with urllib.request.urlopen(req, timeout=10) as resp:
                self.assertEqual(resp.status, 200,
                                 "edit endpoint did not accept new pre_tax value")
            # Verify it landed on disk.
            updated = json.loads(self.REPORT_PATH.read_text())
            stored = updated["transaction_lines"][meal_idx][
                "meal_details"]["pre_tax_amount"]["value"]
            self.assertEqual(stored, new_value,
                             "/edit did not persist the new pre_tax value")
        finally:
            # Restore original on disk so re-runs are idempotent.
            restore_req = urllib.request.Request(
                self.EDIT_URL,
                data=json.dumps(
                    {"path": path, "value": str(original_value)}
                ).encode(),
                headers={"Content-Type": "application/json"},
                method="POST",
            )
            try:
                urllib.request.urlopen(restore_req, timeout=10).close()
            except urllib.error.URLError:
                pass


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(B1_FIXTURE.exists(),
                     f"{B1_FIXTURE} not staged")
@unittest.skipUnless(_flask_reachable(B1_FLASK_URL),
                     f"Flask not reachable at {B1_FLASK_URL}")
class TestWorkbenchBrowserB1Flows(unittest.TestCase):
    """End-to-end interaction flows on the live workbench: edit, delete,
    undo, evidence toggle, issue jump, CSV downloads. Each test
    restores any mutated state in tearDown so re-runs don't drift."""

    URL = f"{B1_FLASK_URL}/uploads/b1-eye/workbench.html"
    EDIT_URL = f"{B1_FLASK_URL}/uploads/b1-eye/edit"
    UNDO_URL = f"{B1_FLASK_URL}/uploads/b1-eye/undo"
    DELETE_URL = f"{B1_FLASK_URL}/uploads/b1-eye/delete-line"
    REPORT_PATH = B1_FIXTURE / "reduced" / "report.json"
    WORKBENCH_PATH = B1_FIXTURE / "workbench.html"
    EDIT_HISTORY_PATH = B1_FIXTURE / "edit_history.json"
    RENDER_BIN = REPO_ROOT / "target" / "debug" / "render_workbench_from_report"

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)
        # Snapshot so setUp can restore between tests (the delete +
        # edit tests both mutate the fixture).
        cls._original_report = cls.REPORT_PATH.read_text()

    @classmethod
    def tearDownClass(cls):
        cls.REPORT_PATH.write_text(cls._original_report)
        cls._render_workbench()
        if cls.EDIT_HISTORY_PATH.exists():
            cls.EDIT_HISTORY_PATH.unlink()
        cls._browser.close()
        cls._playwright.stop()

    @classmethod
    def _render_workbench(cls):
        """Re-render workbench HTML + CSVs from the current report.json."""
        subprocess.run(
            [
                str(cls.RENDER_BIN),
                "--report", str(cls.REPORT_PATH),
                "--receipts-dir", str(B1_FIXTURE / "extractions"),
                "--out", str(cls.WORKBENCH_PATH),
                "--csv-domestic-out", str(B1_FIXTURE / "lines-domestic.csv"),
                "--csv-foreign-out", str(B1_FIXTURE / "lines-foreign.csv"),
            ],
            check=True,
            capture_output=True,
        )

    def setUp(self):
        # Reset the fixture so tests run order-independent.
        self.REPORT_PATH.write_text(self._original_report)
        if self.EDIT_HISTORY_PATH.exists():
            self.EDIT_HISTORY_PATH.unlink()
        self._render_workbench()
        self.page = self._browser.new_page(viewport={"width": 1400, "height": 1800})
        self._console_errors: list[str] = []
        self.page.on("console", lambda msg: (
            self._console_errors.append(msg.text)
            if msg.type == "error" else None
        ))

    def tearDown(self):
        self.page.close()

    def _load(self):
        self.page.goto(self.URL, wait_until="networkidle")

    def test_edit_save_full_browser_flow_updates_value(self):
        """Type new value, press Enter, page reloads, new value visible."""
        self._load()
        path = "expense_report.transaction_lines[2].meal_details.tax_amount"
        original = json.loads(self.REPORT_PATH.read_text())[
            "transaction_lines"][2]["meal_details"]["tax_amount"]["value"]
        new_value = round(original + 0.55, 2)
        try:
            card = self.page.query_selector(f'[data-path="{path}"]')
            self.assertIsNotNone(card, f"card for {path} not found")
            card.query_selector(".field-value").click()
            input_el = card.query_selector("input.inline-edit-input")
            self.assertIsNotNone(input_el)
            input_el.fill(str(new_value))
            # Sync on the /edit POST response so we don't race the
            # double-navigation (location.href hash + reload).
            with self.page.expect_response(
                lambda r: "/edit" in r.url and r.request.method == "POST",
                timeout=10000,
            ):
                input_el.press("Enter")
            self.page.wait_for_function(
                f"() => {{ const el = document.querySelector("
                f"'[data-path=\"{path}\"] .field-value'); "
                f"return el && !el.textContent.includes('Saving'); }}",
                timeout=10000,
            )
            displayed = self.page.query_selector(
                f'[data-path="{path}"] .field-value').text_content()
            self.assertIn(f"{new_value}", displayed,
                          f"page did not show new value after reload: got {displayed!r}")
        finally:
            urllib.request.urlopen(
                urllib.request.Request(
                    self.EDIT_URL,
                    data=json.dumps({"path": path, "value": str(original)}).encode(),
                    headers={"Content-Type": "application/json"},
                    method="POST",
                ), timeout=10,
            ).close()

    def test_delete_line_removes_line_from_dom_after_reload(self):
        """Delete button + confirm dialog + reload removes the line."""
        self._load()
        before = self.page.query_selector_all("details.line-card")
        self.assertEqual(len(before), 3, "fixture should have 3 lines")
        self.page.on("dialog", lambda d: d.accept())
        btn = self.page.query_selector(
            'button.line-delete[data-line-idx="0"]')
        self.assertIsNotNone(btn, "delete button for line 0 missing")
        with self.page.expect_navigation(wait_until="networkidle"):
            btn.click()
        after = self.page.query_selector_all("details.line-card")
        self.assertEqual(len(after), 2,
                         "expected one fewer line after delete")
        amounts = " ".join(
            (el.text_content() or "")
            for el in self.page.query_selector_all(".line-amount")
        )
        self.assertNotIn("57.04", amounts,
                         "deleted line's amount should be gone")
        self.REPORT_PATH.write_text(self._original_report)

    def test_undo_button_hidden_before_edits(self):
        """No edit history means the undo button stays hidden."""
        self._load()
        btn = self.page.query_selector("#undo-button")
        self.assertIsNotNone(btn, "undo button should exist in DOM")
        is_hidden = self.page.evaluate(
            "() => document.getElementById('undo-button').hidden")
        self.assertTrue(is_hidden,
                        "undo button should be hidden when no edits exist")

    def test_undo_button_appears_after_edit_then_reverts(self):
        """Edit then reload reveals undo; clicking undo reverts on disk."""
        path = "expense_report.transaction_lines[2].meal_details.tip_amount"
        original = json.loads(self.REPORT_PATH.read_text())[
            "transaction_lines"][2]["meal_details"]["tip_amount"]["value"]
        try:
            urllib.request.urlopen(
                urllib.request.Request(
                    self.EDIT_URL,
                    data=json.dumps(
                        {"path": path, "value": str(original + 1.0)}).encode(),
                    headers={"Content-Type": "application/json"},
                    method="POST",
                ), timeout=10,
            ).close()
            self._load()
            self.page.wait_for_function(
                "() => !document.getElementById('undo-button').hidden",
                timeout=5000,
            )
            with self.page.expect_navigation(wait_until="networkidle"):
                self.page.query_selector("#undo-button").click()
            reverted = json.loads(self.REPORT_PATH.read_text())[
                "transaction_lines"][2]["meal_details"]["tip_amount"]["value"]
            self.assertEqual(reverted, original,
                             "undo did not restore the original value")
        finally:
            self.REPORT_PATH.write_text(self._original_report)

    def test_evidence_toggle_adds_body_class_on_check(self):
        """Clicking the evidence toggle adds .show-evidence to <body>."""
        self._load()
        # Default state: body should NOT have .show-evidence.
        has_class_initial = self.page.evaluate(
            "() => document.body.classList.contains('show-evidence')")
        self.assertFalse(has_class_initial,
                         "evidence should be hidden by default")
        self.page.click("#evidence-toggle")
        has_class_on = self.page.evaluate(
            "() => document.body.classList.contains('show-evidence')")
        self.assertTrue(has_class_on,
                        "checking toggle should add .show-evidence to body")
        # Toggle off again.
        self.page.click("#evidence-toggle")
        has_class_off = self.page.evaluate(
            "() => document.body.classList.contains('show-evidence')")
        self.assertFalse(has_class_off,
                         "unchecking toggle should remove .show-evidence")

    def test_issue_jump_link_highlights_target_card(self):
        """Clicking an issue-rail jump link adds .field-card.active to one card."""
        self._load()
        jumps = self.page.query_selector_all("a.issue-jump")
        self.assertGreater(len(jumps), 0,
                           "expected issue-jump links in the issues rail")
        jumps[0].click()
        self.page.wait_for_function(
            "() => document.querySelectorAll('.field-card.active').length === 1",
            timeout=2000,
        )
        active_count = self.page.evaluate(
            "() => document.querySelectorAll('.field-card.active').length")
        self.assertEqual(active_count, 1,
                         "issue-jump should highlight exactly one card")

    def test_csv_download_links_serve_csv_content(self):
        """Each CSV download link resolves to a real CSV body (not 404 / HTML)."""
        self._load()
        links = self.page.query_selector_all('a.download-link[href$=".csv"]')
        self.assertGreater(len(links), 0, "expected CSV download links")
        for link in links:
            href = link.get_attribute("href")
            self.assertIsNotNone(href, "link missing href")
            full_url = urllib.parse.urljoin(self.URL, href)
            with urllib.request.urlopen(full_url, timeout=5) as resp:
                self.assertEqual(resp.status, 200,
                                 f"CSV link {href} returned {resp.status}")
                body = resp.read().decode()
                self.assertIn(",", body.split("\n")[0],
                              f"CSV at {href} body doesn't look like CSV: "
                              f"{body[:120]!r}")


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(_flask_reachable(B1_FLASK_URL),
                     f"Flask not reachable at {B1_FLASK_URL}")
class TestUploadFormBrowser(unittest.TestCase):
    """Upload form rendering, localStorage form-draft persistence,
    and refresh resilience. Each test runs in a fresh context so
    localStorage starts empty."""

    HOME_URL = f"{B1_FLASK_URL}/"
    WORKBENCH_URL = f"{B1_FLASK_URL}/uploads/b1-eye/workbench.html"
    STORAGE_KEY = FORM_DRAFT_KEY

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def setUp(self):
        self.context = self._browser.new_context()
        self.page = self.context.new_page()

    def tearDown(self):
        self.context.close()

    def test_upload_form_loads_with_all_fa_fields(self):
        """Every named FA field renders + the form POSTs to /upload."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        for name in [
            "fa_payee_name", "fa_payee_sunet", "fa_payee_affiliation",
            "fa_event_name", "fa_payment_method",
            "fa_bp_who", "fa_bp_what", "fa_bp_when_from", "fa_bp_when_to",
            "fa_bp_where", "fa_bp_why",
            "file_0", "kind_0",
        ]:
            el = self.page.query_selector(f'[name="{name}"]')
            self.assertIsNotNone(el, f"form field [name={name}] missing")
        form = self.page.query_selector('form[action="/upload"]')
        self.assertIsNotNone(form, "form should POST to /upload")

    def test_text_fields_persist_across_reload(self):
        """Typed text values restore after a page reload via localStorage."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.page.fill('[name="fa_event_name"]', "ASPLOS 2026")
        self.page.fill('[name="fa_bp_who"]', "Aditya Sriram")
        self.page.fill('[name="fa_bp_what"]', "Conference travel")
        self.page.wait_for_timeout(500)  # save handler debounces 300ms
        self.page.reload(wait_until="domcontentloaded")
        self.assertEqual(
            self.page.input_value('[name="fa_event_name"]'), "ASPLOS 2026")
        self.assertEqual(
            self.page.input_value('[name="fa_bp_who"]'), "Aditya Sriram")
        self.assertEqual(
            self.page.input_value('[name="fa_bp_what"]'), "Conference travel")

    def test_date_fields_persist_across_reload(self):
        """Date inputs round-trip through localStorage on reload."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.page.fill('[name="fa_bp_when_from"]', "2026-03-21")
        self.page.fill('[name="fa_bp_when_to"]', "2026-04-04")
        self.page.wait_for_timeout(500)
        self.page.reload(wait_until="domcontentloaded")
        self.assertEqual(
            self.page.input_value('[name="fa_bp_when_from"]'), "2026-03-21")
        self.assertEqual(
            self.page.input_value('[name="fa_bp_when_to"]'), "2026-04-04")

    def test_file_input_not_persisted_for_security(self):
        """The persistence code excludes file inputs (browser security)."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.page.fill('[name="fa_event_name"]', "test event")
        self.page.wait_for_timeout(500)
        snapshot_raw = self.page.evaluate(
            f"() => localStorage.getItem('{self.STORAGE_KEY}')")
        self.assertIsNotNone(snapshot_raw,
                             "localStorage should have form draft")
        snapshot = json.loads(snapshot_raw)
        self.assertNotIn("file_0", snapshot,
                         "file inputs must not be persisted")
        self.assertNotIn("kind_0", snapshot)
        self.assertIn("fa_event_name", snapshot)

    def test_persistence_survives_navigation_away_and_back(self):
        """Typed fields survive a navigate-away-and-back round-trip."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.page.fill('[name="fa_event_name"]', "Persistent Event Name")
        self.page.wait_for_timeout(500)
        self.page.goto(self.WORKBENCH_URL, wait_until="domcontentloaded")
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.assertEqual(
            self.page.input_value('[name="fa_event_name"]'),
            "Persistent Event Name",
        )

    def test_workbench_url_is_GETable_and_reloads_cleanly(self):
        """Refresh-on-workbench should be a plain GET (no POST replay).
        The workbench URL is reloadable without re-POSTing /upload."""
        self.page.goto(self.WORKBENCH_URL, wait_until="domcontentloaded")
        # Capture all requests during reload to ensure no POST to
        # /upload happens.
        post_requests: list[str] = []
        self.page.on("request", lambda req: (
            post_requests.append(req.url)
            if req.method == "POST" and "/upload" in req.url
            else None
        ))
        self.page.reload(wait_until="domcontentloaded")
        self.assertEqual(post_requests, [],
                         "refresh on workbench must not re-POST to /upload")

    def test_workbench_url_returns_200_for_direct_navigation(self):
        """Independent of an in-memory upload session, a workbench URL
        should serve cleanly when the FA opens it from a bookmark or
        a shared link days later. Verifies Flask's static-style serve
        of /uploads/<id>/workbench.html."""
        response_status: list[int] = []
        self.page.on("response", lambda resp: (
            response_status.append(resp.status)
            if resp.url == self.WORKBENCH_URL
            else None
        ))
        self.page.goto(self.WORKBENCH_URL, wait_until="domcontentloaded")
        self.assertTrue(
            any(s == 200 for s in response_status),
            f"workbench URL should return 200; saw {response_status}",
        )

    def test_add_file_button_creates_new_input_row(self):
        """Clicking '+ Add another file' inserts file_N + kind_N inputs."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        for _ in range(3):
            self.page.click('button:has-text("+ Add another file")')
        for i in range(4):  # 1 baseline + 3 added
            self.assertIsNotNone(
                self.page.query_selector(f'[name="file_{i}"]'),
                f"file_{i} input missing after +Add",
            )
            self.assertIsNotNone(
                self.page.query_selector(f'[name="kind_{i}"]'),
                f"kind_{i} input missing after +Add",
            )

    def test_kind_dropdown_lists_every_extractor(self):
        """The kind dropdown enumerates every registered extractor kind."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        values = self.page.eval_on_selector_all(
            '[name="kind_0"] option',
            "els => els.map(e => e.value)",
        )
        for kind in ("meal", "transport", "lodging", "airfare",
                     "miscellaneous", "membership", "mileage"):
            self.assertIn(kind, values,
                          f"kind '{kind}' missing from upload dropdown")

    def test_required_field_blocks_submit(self):
        """Empty required field makes the form fail HTML5 validation."""
        self.page.goto(self.HOME_URL, wait_until="domcontentloaded")
        self.page.fill('[name="fa_payee_name"]', "")
        is_valid = self.page.evaluate(
            '() => document.querySelector("form[action=\\"/upload\\"]")'
            '.checkValidity()'
        )
        self.assertFalse(is_valid,
                         "form should fail validation with empty required field")

    def test_docx_upload_returns_friendly_400(self):
        """Posting an unsupported file extension returns the 400 page."""
        boundary = "----testboundary"
        body = (
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="fa_payee_name"\r\n\r\n'
            "Test\r\n"
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="fa_payee_sunet"\r\n\r\n'
            "test\r\n"
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="fa_event_name"\r\n\r\n'
            "Test\r\n"
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="file_0"; filename="x.docx"\r\n'
            'Content-Type: application/octet-stream\r\n\r\n'
            "fake content\r\n"
            f"--{boundary}\r\n"
            'Content-Disposition: form-data; name="kind_0"\r\n\r\n'
            "meal\r\n"
            f"--{boundary}--\r\n"
        ).encode()
        req = urllib.request.Request(
            f"{B1_FLASK_URL}/upload",
            data=body,
            headers={"Content-Type": f"multipart/form-data; boundary={boundary}"},
            method="POST",
        )
        try:
            urllib.request.urlopen(req, timeout=10)
            self.fail("expected HTTP 400 for .docx upload")
        except urllib.error.HTTPError as e:
            self.assertEqual(e.code, 400, "expected 400 for .docx")
            body_text = e.read().decode()
            self.assertIn("Unsupported file type", body_text,
                          "expected friendly extension-reject message")


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(B2_FIXTURE.exists(),
                     f"{B2_FIXTURE} not staged — synth-mileage fixture missing")
@unittest.skipUnless(_flask_reachable(B1_FLASK_URL),
                     f"Flask not reachable at {B1_FLASK_URL}")
class TestWorkbenchBrowserB2(unittest.TestCase):
    """Personal-mileage workbench: card render + IRS-rate computation.
    Uses a synthetic per-doc JSON (Stanford to SFO, 32 mi) so the
    reduce + render + JS path runs without a Vertex call."""

    URL = f"{B1_FLASK_URL}/uploads/b2-eye/workbench.html"
    EDIT_URL = f"{B1_FLASK_URL}/uploads/b2-eye/edit"
    REPORT_PATH = B2_FIXTURE / "reduced" / "report.json"

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def setUp(self):
        self.page = self._browser.new_page(viewport={"width": 1400, "height": 1600})
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
        self.page.goto(self.URL, wait_until="networkidle")

    def test_b2_screenshot_for_eyeball(self):
        """Save a full-page screenshot for visual eyeball."""
        self._load()
        out = REPO_ROOT / ".scratch" / "b2-eye" / "workbench-screenshot.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        self.page.screenshot(path=str(out), full_page=True)
        self.assertGreater(out.stat().st_size, 10_000)

    def test_b2_no_javascript_parse_errors(self):
        """A template edit that breaks JS would surface as a page error."""
        self._load()
        self.assertEqual(self._page_errors, [],
                         f"page parsed with JS errors: {self._page_errors}")

    def test_b2_mileage_card_labels_render(self):
        """All four mileage-detail labels show up in the rendered HTML."""
        self._load()
        body = self.page.content()
        for label in ["Distance", "Origin", "Destination", "Trip Date"]:
            self.assertIn(f">{label}<", body,
                          f"mileage card label '{label}' missing")

    def test_b2_mileage_cards_have_data_path_for_edit(self):
        """All four mileage_details fields are click-to-editable."""
        self._load()
        for suffix in ["distance_miles", "origin", "destination", "trip_date"]:
            els = self.page.query_selector_all(
                f'[data-path$="mileage_details.{suffix}"]'
            )
            self.assertGreater(len(els), 0,
                               f"no data-path element ending in 'mileage_details.{suffix}'")

    def test_b2_distance_renders_with_mi_suffix(self):
        """Distance renders as '32.0 mi' so the unit is unambiguous."""
        self._load()
        card = self.page.query_selector(
            '[data-path$="mileage_details.distance_miles"] .field-value'
        )
        self.assertIsNotNone(card, "distance_miles card missing")
        text = card.text_content() or ""
        self.assertIn("mi", text,
                      f"distance should include 'mi' suffix: {text!r}")
        self.assertIn("32", text)

    def test_b2_computed_line_amount_shows_irs_rate_in_confidence_reason(self):
        """End-to-end: reduce computes 32 mi x $0.70 = $22.40 and stamps
        the rate breakdown into the confidence reason."""
        self._load()
        body = self.page.content()
        self.assertIn("$22.40", body,
                      "computed line_amount_usd of $22.40 not rendered")
        self.assertIn("IRS 2025 business rate", body,
                      "confidence reason should mention the IRS rate basis")
        self.assertIn("$0.700/mi", body,
                      "IRS rate in the confidence reason should match the cache")

    def test_b2_summary_total_matches_derived_line_amount(self):
        """Hero summary picks up the derived per-line amount (not the
        raw extractor's null), so the line and hero both show $22.40."""
        self._load()
        body = self.page.content()
        amount_occurrences = body.count("$22.40")
        self.assertGreaterEqual(amount_occurrences, 2,
                                f"expected $22.40 in both line + hero; "
                                f"found {amount_occurrences} occurrence(s)")


if __name__ == "__main__":
    unittest.main()
