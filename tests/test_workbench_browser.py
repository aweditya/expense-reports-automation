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
import os
import subprocess
import sys
import unittest
import urllib.error
import urllib.parse
import urllib.request
import uuid
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
        """Every named FA field renders, form POSTs to /upload, and
        loading the page produces zero console errors. The console
        check guards against the class of bug where an HTML5 pattern
        attribute (or similar) is invalid under Chromium's /v parser
        and silently disables client-side validation."""
        errs: list[str] = []
        self.page.on("console", lambda msg: (
            errs.append(msg.text) if msg.type == "error" else None))
        self.page.on("pageerror", lambda exc: errs.append(f"PAGEERROR: {exc}"))
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
        self.assertEqual(errs, [],
                         f"unexpected console/page errors on form load: {errs}")

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


PROD_URL = "https://expense-reports-wgnivgelea-uw.a.run.app"


def _prod_e2e_enabled() -> bool:
    """E2E against prod costs real money + time. Opt-in via env var."""
    return os.environ.get("RUN_PROD_E2E") == "1"


def _gcloud_id_token() -> str | None:
    """IAP bearer token for prod. Skip if gcloud isn't set up."""
    try:
        out = subprocess.run(
            ["gcloud", "auth", "print-identity-token"],
            capture_output=True, check=True, timeout=10,
        )
        return out.stdout.decode().strip()
    except Exception:
        return None


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(_prod_e2e_enabled(),
                     "set RUN_PROD_E2E=1 to run the prod E2E flow")
@unittest.skipUnless(_gcloud_id_token() is not None,
                     "gcloud identity token unavailable")
class TestProdFullFAJourney(unittest.TestCase):
    """End-to-end FA flow against deployed prod: form fill, file
    upload, SSE progress, workbench interaction, CSV download.
    Single test method walks the entire journey with screenshots at
    each milestone. ~$0.50 in Vertex + Document AI, ~5min wallclock.
    """

    RECEIPTS = [
        ("meal", "meal_2026-04-04_original-mels-san-leandro.jpeg"),
        ("transport", "transport_2023-01-08_lyft-dtw-to-novi.pdf"),
        ("lodging", "lodging_2023-01-09_warren-hotel-v2.pdf"),
        ("airfare", "airfare_2026-03-21_egencia-united-sfo-pit-roundtrip.pdf"),
        ("membership", "membership_2026-01-29_acm-student-renewal.png"),
    ]
    FA_FIELDS = {
        "fa_payee_name": "E2E Test Payee",
        "fa_payee_sunet": "e2etest",
        "fa_payee_affiliation": "stanford_faculty",
        "fa_event_name": "E2E Journey Test",
        "fa_payment_method": "electronic",
        "fa_rush_processing": "no",
        "fa_authorized_by": "advisor@stanford.edu",
        "fa_bp_who": "E2E Test",
        "fa_bp_what": "End-to-end FA journey verification",
        "fa_bp_when_from": "2026-03-21",
        "fa_bp_when_to": "2026-04-04",
        "fa_bp_where": "Pittsburgh, PA",
        "fa_bp_why": "Verify the FA journey doesn't crash",
    }
    SHOTS_DIR = REPO_ROOT / ".scratch" / "e2e-fa-journey"

    def setUp(self):
        self._playwright = sync_playwright().start()
        self._browser = self._playwright.chromium.launch(headless=True)
        self._token = _gcloud_id_token()
        self.context = self._browser.new_context(
            viewport={"width": 1400, "height": 1800},
            extra_http_headers={"Authorization": f"Bearer {self._token}"},
        )
        self.page = self.context.new_page()
        self._console_errors: list[str] = []
        self.page.on("console", lambda msg: (
            self._console_errors.append(msg.text)
            if msg.type == "error" else None
        ))
        self.SHOTS_DIR.mkdir(parents=True, exist_ok=True)

    def tearDown(self):
        self._browser.close()
        self._playwright.stop()

    def _snap(self, name: str):
        self.page.screenshot(path=str(self.SHOTS_DIR / f"{name}.png"),
                             full_page=True)

    def test_full_fa_journey(self):
        """Form fill → upload → SSE → workbench → edit → CSV download."""
        # 1. Form load
        self.page.goto(PROD_URL + "/", wait_until="domcontentloaded")
        self.assertIsNotNone(self.page.query_selector('form[action="/upload"]'))
        self._snap("01-form-blank")

        # 2. Fill required fields
        for name, value in self.FA_FIELDS.items():
            el = self.page.query_selector(f'[name="{name}"]')
            if el is None:
                continue
            tag = el.evaluate("e => e.tagName")
            if tag == "SELECT":
                self.page.select_option(f'[name="{name}"]', value)
            else:
                self.page.fill(f'[name="{name}"]', value)

        # 3. Add additional file rows
        for _ in range(len(self.RECEIPTS) - 1):
            self.page.click('button:has-text("+ Add another file")')

        # 4. Set files + kinds
        for i, (kind, filename) in enumerate(self.RECEIPTS):
            receipt_path = REPO_ROOT / "receipts" / filename
            self.assertTrue(receipt_path.exists(),
                            f"missing receipt: {receipt_path}")
            self.page.set_input_files(f'[name="file_{i}"]', str(receipt_path))
            self.page.select_option(f'[name="kind_{i}"]', kind)
        self._snap("02-form-filled")

        # 5. Submit + capture upload_id from the progress URL
        with self.page.expect_navigation(wait_until="domcontentloaded",
                                         timeout=60_000):
            self.page.click('button[type="submit"]')
        upload_id = self.page.url.rstrip("/").split("/")[-1]
        self.assertTrue(upload_id, "no upload_id in progress URL")

        # 6. Mid-extract screenshot after a beat
        self.page.wait_for_timeout(20_000)
        self._snap("03-progress-mid")

        # 7. Wait for the JS to redirect to workbench (allow generous time)
        self.page.wait_for_url("**/workbench.html", timeout=900_000)
        self.page.wait_for_load_state("networkidle", timeout=30_000)
        self._snap("04-workbench-loaded")

        # 8. Workbench sanity: N line cards
        line_cards = self.page.query_selector_all("details.line-card")
        self.assertEqual(len(line_cards), len(self.RECEIPTS),
                         f"expected {len(self.RECEIPTS)} line cards, "
                         f"got {len(line_cards)}")

        # 9. Evidence toggle round-trip
        self.page.click("#evidence-toggle")
        self.assertTrue(self.page.evaluate(
            "() => document.body.classList.contains('show-evidence')"))
        self.page.click("#evidence-toggle")
        self.assertFalse(self.page.evaluate(
            "() => document.body.classList.contains('show-evidence')"))

        # 10. Edit a field end-to-end
        edit_url = f"{PROD_URL}/uploads/{upload_id}/edit"
        cards_with_value = self.page.query_selector_all(
            "[data-path] .field-value")
        self.assertGreater(len(cards_with_value), 10)
        # Pick the first venue_name card (meal) which is reliably present
        venue_card = self.page.query_selector(
            '[data-path$="meal_details.venue_name"]')
        if venue_card is not None:
            venue_card.query_selector(".field-value").click()
            input_el = venue_card.query_selector("input.inline-edit-input")
            self.assertIsNotNone(input_el)
            input_el.fill("E2E Edited Venue Name")
            with self.page.expect_response(
                lambda r: "/edit" in r.url and r.request.method == "POST",
                timeout=10_000,
            ):
                input_el.press("Enter")
            # Wait for the value to land in the DOM rather than reading
            # page.content() (which can race the JS-driven reload that
            # follows the /edit POST).
            self.page.wait_for_function(
                "() => document.body.innerText.includes("
                "'E2E Edited Venue Name')",
                timeout=15_000,
            )
        self._snap("05-after-edit")

        # 11. Issues rail jump
        jumps = self.page.query_selector_all("a.issue-jump")
        if jumps:
            jumps[0].click()
            self.page.wait_for_function(
                "() => document.querySelectorAll('.field-card.active').length === 1",
                timeout=2000,
            )

        # 12. CSV download links resolve to real CSV bodies
        links = self.page.query_selector_all('a.download-link[href$=".csv"]')
        self.assertGreater(len(links), 0)
        for link in links:
            href = link.get_attribute("href")
            full_url = urllib.parse.urljoin(self.page.url, href)
            req = urllib.request.Request(
                full_url,
                headers={"Authorization": f"Bearer {self._token}"},
            )
            with urllib.request.urlopen(req, timeout=10) as resp:
                self.assertEqual(resp.status, 200,
                                 f"CSV link {href} returned {resp.status}")
                body = resp.read().decode()
                self.assertIn(",", body.split("\n")[0],
                              f"CSV at {href} looks empty: {body[:100]!r}")

        # 13. Refresh-on-workbench does not re-POST /upload
        posts: list[str] = []
        self.page.on("request", lambda req: (
            posts.append(req.url)
            if req.method == "POST" and "/upload" in req.url
            else None
        ))
        self.page.reload(wait_until="domcontentloaded")
        self.assertEqual(posts, [], "refresh re-POSTed /upload")
        self._snap("06-final")

        # 14. No unexpected console errors during the journey
        unexpected = [e for e in self._console_errors
                      if "undo-available" not in e]
        self.assertEqual(unexpected, [],
                         f"unexpected console errors during journey: {unexpected}")


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(_prod_e2e_enabled(),
                     "set RUN_PROD_E2E=1 to run prod failure-mode tests")
@unittest.skipUnless(_gcloud_id_token() is not None,
                     "gcloud identity token unavailable")
class TestProdFailureModes(unittest.TestCase):
    """Failure-mode coverage against prod: returning FA, concurrent
    tabs, non-ASCII filenames. Each test uses one cheap receipt
    (membership PNG) to keep cost low. ~$0.10 + ~4min per test."""

    RECEIPT = ("membership",
               "membership_2026-01-29_acm-student-renewal.png")
    FA_FIELDS = {
        "fa_payee_name": "Failure Mode Test",
        "fa_payee_sunet": "fmtest",
        "fa_payee_affiliation": "stanford_faculty",
        "fa_event_name": "Failure Mode Coverage",
        "fa_payment_method": "electronic",
        "fa_rush_processing": "no",
        "fa_authorized_by": "advisor@stanford.edu",
        "fa_bp_who": "Failure Mode Test",
        "fa_bp_what": "Coverage of FA-returns / concurrent-tab / unicode-filename",
        "fa_bp_when_from": "2026-01-29",
        "fa_bp_when_to": "2026-01-29",
        "fa_bp_where": "Stanford, CA",
        "fa_bp_why": "Confirm system handles real-world edge cases",
    }
    SHOTS_DIR = REPO_ROOT / ".scratch" / "e2e-failure-modes"

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)
        cls._token = _gcloud_id_token()
        cls.SHOTS_DIR.mkdir(parents=True, exist_ok=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def _new_context(self):
        return self._browser.new_context(
            viewport={"width": 1400, "height": 1800},
            extra_http_headers={"Authorization": f"Bearer {self._token}"},
        )

    def _submit_one(self, page, file_path: Path, kind: str) -> str:
        """Fill form, attach one receipt, submit, return upload_id."""
        page.goto(PROD_URL + "/", wait_until="domcontentloaded")
        for name, value in self.FA_FIELDS.items():
            el = page.query_selector(f'[name="{name}"]')
            if el is None:
                continue
            tag = el.evaluate("e => e.tagName")
            if tag == "SELECT":
                page.select_option(f'[name="{name}"]', value)
            else:
                page.fill(f'[name="{name}"]', value)
        page.set_input_files('[name="file_0"]', str(file_path))
        page.select_option('[name="kind_0"]', kind)
        with page.expect_navigation(wait_until="domcontentloaded",
                                    timeout=60_000):
            page.click('button[type="submit"]')
        return page.url.rstrip("/").split("/")[-1]

    def test_refresh_during_progress_page_recovers(self):
        """Stage 11 claim: refreshing the progress page mid-extract
        does NOT crash the website. The page re-renders (it's a plain
        idempotent GET) and the EventSource reconnects to current
        phase. Test: start an upload, wait for the status URL, sleep
        a beat to land inside extract phase, reload, then keep
        waiting for the workbench redirect."""
        ctx = self._new_context()
        page = ctx.new_page()
        receipt = REPO_ROOT / "receipts" / self.RECEIPT[1]
        upload_id = self._submit_one(page, receipt, self.RECEIPT[0])
        # We should now be on /upload/status/<id>. Wait a beat so the
        # pipeline ticks into extract phase, then refresh.
        self.assertIn(f"/upload/status/{upload_id}", page.url,
                      f"unexpected url after submit: {page.url}")
        page.wait_for_timeout(5_000)
        page.reload(wait_until="domcontentloaded")
        # After reload we should STILL be on the status page (or have
        # already advanced to workbench if extract was fast).
        self.assertTrue(
            f"/upload/status/{upload_id}" in page.url
            or "workbench.html" in page.url,
            f"refresh landed somewhere unexpected: {page.url}")
        # Confirm the page reconnected — it should redirect to the
        # workbench when extraction completes, not hang.
        page.wait_for_url("**/workbench.html", timeout=600_000)
        line_cards = page.query_selector_all("details.line-card")
        self.assertGreaterEqual(len(line_cards), 1,
                                "post-refresh workbench shows no lines")
        ctx.close()

    def test_returning_fa_to_completed_workbench(self):
        """Gap 1a. After upload completes, an FA returning later in a
        FRESH browser context (no localStorage, no cookies, no warm
        page state) can still load the workbench at
        /uploads/<id>/workbench.html. Validates the static-file path
        survives container restart / Firestore-only JOBS state."""
        ctx_a = self._new_context()
        page_a = ctx_a.new_page()
        receipt = REPO_ROOT / "receipts" / self.RECEIPT[1]
        upload_id = self._submit_one(page_a, receipt, self.RECEIPT[0])
        page_a.wait_for_url("**/workbench.html", timeout=600_000)
        ctx_a.close()

        ctx_b = self._new_context()
        page_b = ctx_b.new_page()
        page_b.goto(f"{PROD_URL}/uploads/{upload_id}/workbench.html",
                    wait_until="networkidle", timeout=60_000)
        line_cards = page_b.query_selector_all("details.line-card")
        self.assertGreaterEqual(len(line_cards), 1,
                                "returning FA sees no line cards on workbench")
        page_b.screenshot(path=str(self.SHOTS_DIR / "1a-returning-fa.png"),
                          full_page=True)
        ctx_b.close()

    def test_returning_fa_reconnects_to_in_flight_progress(self):
        """Gap 1b. FA closes laptop mid-upload, opens later. New
        context revisits /upload/status/<id>; SSE picks up current
        phase from Firestore and the page completes (redirects to
        workbench). Validates Firestore-backed JOBS survives session
        boundary."""
        ctx_a = self._new_context()
        page_a = ctx_a.new_page()
        receipt = REPO_ROOT / "receipts" / self.RECEIPT[1]
        upload_id = self._submit_one(page_a, receipt, self.RECEIPT[0])
        status_url = f"{PROD_URL}/upload/status/{upload_id}"
        # Beat to let the pipeline tick into extract phase, then walk
        # away.
        page_a.wait_for_timeout(8_000)
        ctx_a.close()

        ctx_b = self._new_context()
        page_b = ctx_b.new_page()
        page_b.goto(status_url, wait_until="domcontentloaded",
                    timeout=30_000)
        page_b.wait_for_url("**/workbench.html", timeout=600_000)
        line_cards = page_b.query_selector_all("details.line-card")
        self.assertGreaterEqual(len(line_cards), 1,
                                "reconnected FA sees no line cards")
        page_b.screenshot(path=str(self.SHOTS_DIR / "1b-reconnect.png"),
                          full_page=True)
        ctx_b.close()

    def test_two_concurrent_tabs_same_fa(self):
        """Gap 3. Same FA opens two browser tabs, submits two distinct
        uploads near-simultaneously. Both must reach workbench
        cleanly without JOBS cross-contamination."""
        ctx1 = self._new_context()
        ctx2 = self._new_context()
        page1 = ctx1.new_page()
        page2 = ctx2.new_page()
        receipt = REPO_ROOT / "receipts" / self.RECEIPT[1]
        id1 = self._submit_one(page1, receipt, self.RECEIPT[0])
        id2 = self._submit_one(page2, receipt, self.RECEIPT[0])
        self.assertNotEqual(id1, id2,
                            "server generated identical upload_ids")
        page1.wait_for_url("**/workbench.html", timeout=600_000)
        page2.wait_for_url("**/workbench.html", timeout=600_000)
        self.assertIn(id1, page1.url)
        self.assertIn(id2, page2.url)
        self.assertGreaterEqual(
            len(page1.query_selector_all("details.line-card")), 1)
        self.assertGreaterEqual(
            len(page2.query_selector_all("details.line-card")), 1)
        ctx1.close()
        ctx2.close()

    def test_gcs_artifacts_land_after_upload(self):
        """Durable-store Phase 2b. After upload, the source PDF +
        the per-receipt extraction JSON are in GCS. Dual-write is
        firing in prod."""
        try:
            from gcs_artifacts import list_artifacts
        except ImportError:
            self.skipTest("gcs_artifacts module not importable")

        ctx = self._new_context()
        page = ctx.new_page()
        receipt = REPO_ROOT / "receipts" / self.RECEIPT[1]
        upload_id = self._submit_one(page, receipt, self.RECEIPT[0])
        page.wait_for_url("**/workbench.html", timeout=600_000)
        ctx.close()

        files = list_artifacts(upload_id, "files")
        extractions = list_artifacts(upload_id, "extractions")
        self.assertIn(self.RECEIPT[1], files,
                      f"source file missing from GCS — got {files}")
        # The extraction JSON is named after the source's stem.
        stem = Path(self.RECEIPT[1]).stem
        self.assertIn(f"{stem}.json", extractions,
                      f"extraction JSON missing from GCS — got {extractions}")

    def test_firestore_report_persists_after_upload(self):
        """Durable-store Phase 2a. After an upload completes, the
        reports/{upload_id} Firestore doc exists with the report
        payload + fa_input + (empty) history. Validates the
        dual-write path is firing in prod."""
        try:
            from firestore_reports import get_report
        except ImportError:
            self.skipTest("firestore_reports module not importable")

        ctx = self._new_context()
        page = ctx.new_page()
        receipt = REPO_ROOT / "receipts" / self.RECEIPT[1]
        upload_id = self._submit_one(page, receipt, self.RECEIPT[0])
        page.wait_for_url("**/workbench.html", timeout=600_000)
        ctx.close()

        doc = get_report(upload_id)
        self.assertIsNotNone(doc,
                             f"Firestore reports/{upload_id} missing — "
                             f"Phase 2a dual-write didn't fire")
        report = doc["report"]
        self.assertIn("transaction_lines", report,
                      "report payload missing transaction_lines root key")
        self.assertGreaterEqual(len(report["transaction_lines"]), 1,
                                "uploaded receipt should produce ≥1 line")
        self.assertIsNotNone(doc["fa_input"],
                             "fa_input should be populated from upload form")
        self.assertEqual(doc["fa_input"].get("payee_sunet"),
                         self.FA_FIELDS["fa_payee_sunet"],
                         "fa_input JSON uses stripped keys "
                         "(payee_sunet, not fa_payee_sunet) "
                         "per write_fa_input mapping")
        self.assertEqual(doc["history"], [],
                         "fresh upload should have empty edit history")

    def test_add_receipts_appends_to_existing_report(self):
        """Full add-receipts UI flow against prod. Upload 1 receipt,
        click '+ Add more receipts' on the workbench, attach another
        receipt of a different kind, submit, wait for the redirect
        back to the (now combined) workbench, assert both lines are
        present. Also asserts Firestore + GCS picked up the new
        artifacts. Walks the entire return-tomorrow FA workflow."""
        ctx = self._new_context()
        page = ctx.new_page()
        first = REPO_ROOT / "receipts" / self.RECEIPT[1]
        upload_id = self._submit_one(page, first, self.RECEIPT[0])
        page.wait_for_url("**/workbench.html", timeout=600_000)
        self.assertEqual(
            len(page.query_selector_all("details.line-card")), 1,
            "should start with one line from the first upload")

        # Open the add-receipts modal + add a different-kind receipt.
        page.click("#add-receipts-button")
        page.wait_for_selector("#add-receipts-modal:not(.hidden)",
                               timeout=5000)
        second = REPO_ROOT / "receipts" / "airfare_2026-03-21_egencia-united-sfo-pit-roundtrip.pdf"
        self.assertTrue(second.exists(), f"missing second receipt: {second}")
        page.set_input_files('[name="file_0"]', str(second))
        page.select_option('[name="kind_0"]', "airfare")
        page.click("#add-receipts-submit")

        # Server 303s the fetch to /upload/status/<id>; the JS sets
        # window.location.href and the status page eventually redirects
        # to the rebuilt workbench.
        page.wait_for_url("**/upload/status/**", timeout=30_000)
        page.wait_for_url("**/workbench.html", timeout=900_000)
        line_cards = page.query_selector_all("details.line-card")
        self.assertEqual(len(line_cards), 2,
                         f"after add-receipts should see 2 lines; "
                         f"got {len(line_cards)}")
        page.screenshot(path=str(self.SHOTS_DIR / "add-receipts-combined.png"),
                        full_page=True)
        ctx.close()

        # Durable proof: Firestore + GCS picked up both.
        try:
            from firestore_reports import get_report
            from gcs_artifacts import list_artifacts
            doc = get_report(upload_id)
            self.assertIsNotNone(doc)
            self.assertEqual(
                len(doc["report"].get("transaction_lines", [])), 2)
            self.assertEqual(doc["history"], [],
                             "edit history should be cleared after add")
            files = list_artifacts(upload_id, "files")
            self.assertEqual(len(files), 2,
                             f"both source files should be in GCS; got {files}")
            extractions = list_artifacts(upload_id, "extractions")
            self.assertEqual(len(extractions), 2)
        except ImportError:
            pass  # SDK absent; UI assertions stand alone

    def test_edits_persist_across_browser_sessions(self):
        """Phase 2a end-to-end persistence proof. Edit a field in
        context A, close it, open a FRESH context B (no cookies,
        no localStorage), reload the same workbench URL, assert
        the edit is visible. Proves the edit flowed through Firestore
        (Stage 2a dual-write) AND survives across browser sessions."""
        ctx_a = self._new_context()
        page_a = ctx_a.new_page()
        receipt = REPO_ROOT / "receipts" / self.RECEIPT[1]
        upload_id = self._submit_one(page_a, receipt, self.RECEIPT[0])
        page_a.wait_for_url("**/workbench.html", timeout=600_000)

        # Edit remarks on line 0 to a unique sentinel string. Remarks
        # is universally present + safely free-form across kinds.
        sentinel = f"PERSIST-PROOF-{uuid.uuid4().hex[:8]}"
        remarks_card = page_a.query_selector(
            '[data-path$="remarks"]')
        self.assertIsNotNone(remarks_card,
                             "expected a remarks field card on the workbench")
        remarks_card.query_selector(".field-value").click()
        input_el = remarks_card.query_selector("input.inline-edit-input")
        self.assertIsNotNone(input_el)
        input_el.fill(sentinel)
        with page_a.expect_response(
            lambda r: "/edit" in r.url and r.request.method == "POST",
            timeout=15_000,
        ):
            input_el.press("Enter")
        page_a.wait_for_function(
            f"() => document.body.innerText.includes({json.dumps(sentinel)})",
            timeout=15_000,
        )
        ctx_a.close()

        # Fresh context — separate browser_context, no cookies/storage.
        ctx_b = self._new_context()
        page_b = ctx_b.new_page()
        page_b.goto(f"{PROD_URL}/uploads/{upload_id}/workbench.html",
                    wait_until="networkidle", timeout=60_000)
        body_text = page_b.evaluate("() => document.body.innerText")
        self.assertIn(sentinel, body_text,
                      f"edit {sentinel!r} not visible in fresh-context reload")
        ctx_b.close()

        # Also assert Firestore sees the edit (proves dual-write
        # actually fired, not just disk cache).
        try:
            from firestore_reports import get_report
            doc = get_report(upload_id)
            self.assertIsNotNone(doc, "Firestore doc missing after edit")
            report_str = json.dumps(doc["report"])
            self.assertIn(sentinel, report_str,
                          "edit not persisted to Firestore "
                          "— dual-write must have failed silently")
            self.assertEqual(len(doc["history"]), 1,
                             "history should record the one edit")
        except ImportError:
            pass  # gcloud-firestore not installed in this env; UI check stood

    def test_non_ascii_filename_sanitized(self):
        """Gap 4a. Upload a receipt with a non-ASCII filename
        (emoji + Chinese). sanitize_filename strips it to safe ASCII;
        the upload must still complete and the workbench must render."""
        import shutil
        src = REPO_ROOT / "receipts" / self.RECEIPT[1]
        scratch = REPO_ROOT / ".scratch" / "e2e-failure-modes"
        scratch.mkdir(parents=True, exist_ok=True)
        unicode_path = scratch / "receipt_2026年度_测试_✈.png"
        shutil.copy(src, unicode_path)

        ctx = self._new_context()
        page = ctx.new_page()
        upload_id = self._submit_one(page, unicode_path, self.RECEIPT[0])
        page.wait_for_url("**/workbench.html", timeout=600_000)
        line_cards = page.query_selector_all("details.line-card")
        self.assertGreaterEqual(len(line_cards), 1,
                                "unicode-filename upload produced no line cards")
        page.screenshot(path=str(self.SHOTS_DIR / "4a-unicode.png"),
                        full_page=True)
        ctx.close()


_STAGE2C_FIXTURE = (REPO_ROOT / ".scratch" / "uploads"
                     / "2026-05-23_00-11-46_14f50398")


def _stage2c_reachable() -> bool:
    """Stage 2c local test needs Firestore + GCS reachable + a real
    fixture upload to mirror into them + Playwright + the render
    binary. Each pre-req gates the suite cleanly."""
    if not PLAYWRIGHT_AVAILABLE:
        return False
    if not _STAGE2C_FIXTURE.exists():
        return False
    if not (REPO_ROOT / "target" / "debug"
            / "render_workbench_from_report").exists():
        return False
    if not os.environ.get("VERTEX_PROJECT_ID"):
        return False
    try:
        from firestore_reports import _get_client as _fs
        from gcs_artifacts import _get_client as _gcs, BUCKET_NAME
        _fs()
        return _gcs().bucket(BUCKET_NAME).exists()
    except Exception:
        return False


@unittest.skipUnless(_stage2c_reachable(),
                     "Stage 2c local test needs Playwright, the render "
                     "binary, the existing local fixture, and live "
                     "Firestore + GCS via ADC")
class TestStage2cRehydrate(unittest.TestCase):
    """Stage 2c. Verifies _rehydrate_upload + the on-cache-miss route:
    pre-stage Firestore + GCS for a synthetic upload_id using an
    existing local fixture, blow away the local dir, call rehydrate,
    then load workbench.html in Chromium and assert it parses + has
    line cards + zero console errors.

    Mirrors what happens in prod when a container recycle wipes
    /scratch/uploads/<id>/ but the durable state survives."""

    @classmethod
    def setUpClass(cls):
        import sys as _sys
        _sys.path.insert(0, str(REPO_ROOT / "scripts"))
        from firestore_reports import set_report  # noqa: E402
        from gcs_artifacts import upload_artifact  # noqa: E402

        cls.upload_id = f"test_2c_{uuid.uuid4().hex[:10]}"
        report = json.loads(
            (_STAGE2C_FIXTURE / "reduced" / "report.json").read_text())
        set_report(cls.upload_id, report=report)
        for f in (_STAGE2C_FIXTURE / "files").iterdir():
            upload_artifact(cls.upload_id, "files", f)
        for f in (_STAGE2C_FIXTURE / "extractions").iterdir():
            upload_artifact(cls.upload_id, "extractions", f)

        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)

    @classmethod
    def tearDownClass(cls):
        try:
            cls._browser.close()
            cls._playwright.stop()
        except Exception:
            pass
        try:
            from firestore_reports import delete_report
            from gcs_artifacts import delete_artifacts
            delete_report(cls.upload_id)
            delete_artifacts(cls.upload_id)
        except Exception:
            pass
        import shutil
        shutil.rmtree(REPO_ROOT / ".scratch" / "uploads" / cls.upload_id,
                      ignore_errors=True)

    def test_rehydrate_rebuilds_disk_then_workbench_loads_in_chromium(self):
        import importlib
        import sys as _sys
        _sys.path.insert(0, str(REPO_ROOT / "scripts"))
        local_app_simple = importlib.import_module("local_app_simple")
        # Flip the gates at runtime — module-level constants were
        # read at import time before env was set.
        local_app_simple.USE_FIRESTORE_REPORTS = True
        local_app_simple.USE_GCS_ARTIFACTS = True

        upload_dir = (REPO_ROOT / ".scratch" / "uploads" / self.upload_id)
        import shutil
        shutil.rmtree(upload_dir, ignore_errors=True)
        self.assertFalse(upload_dir.exists(),
                         "local upload_dir should be absent before rehydrate")

        ok = local_app_simple._rehydrate_upload(self.upload_id, upload_dir)
        self.assertTrue(ok, "rehydrate must succeed when Firestore has the doc")

        # Disk shape — every artifact the workbench JS depends on.
        self.assertTrue((upload_dir / "reduced" / "report.json").exists())
        self.assertTrue((upload_dir / "workbench.html").exists())
        files = sorted(p.name for p in (upload_dir / "files").iterdir())
        extractions = sorted(p.name for p in (upload_dir / "extractions").iterdir())
        self.assertGreater(len(files), 0,
                           "source files should have been pulled from GCS")
        self.assertGreater(len(extractions), 0,
                           "extraction JSONs should have been pulled from GCS")

        # Browser eyeball: the rehydrated workbench must actually load
        # cleanly. Catches malformed HTML / missing JS bundles / etc.
        ctx = self._browser.new_context()
        page = ctx.new_page()
        console_errors: list[str] = []
        page.on("console", lambda msg: (
            console_errors.append(msg.text)
            if msg.type == "error" else None))
        workbench_url = (upload_dir / "workbench.html").as_uri()
        page.goto(workbench_url, wait_until="networkidle")
        line_cards = page.query_selector_all("details.line-card")
        self.assertGreater(len(line_cards), 0,
                           "rehydrated workbench should render line cards")
        unexpected = [e for e in console_errors
                      if "undo-available" not in e
                      and "favicon" not in e]
        self.assertEqual(unexpected, [],
                         f"console errors on rehydrated workbench: {unexpected}")
        ctx.close()


@unittest.skipUnless(PLAYWRIGHT_AVAILABLE,
                     "playwright not installed")
@unittest.skipUnless(_prod_e2e_enabled(),
                     "set RUN_PROD_E2E=1 to run prod foreign-receipt tests")
@unittest.skipUnless(_gcloud_id_token() is not None,
                     "gcloud identity token unavailable")
class TestProdForeignReceipts(unittest.TestCase):
    """Multilingual + FX end-to-end in prod. Each test uploads ONE
    real foreign-language receipt and asserts the extracted line has
    a non-USD original currency, a populated USD amount (Frankfurter
    FX landed), and a data row in lines-foreign.csv. Catches
    multilingual extraction regressions AND silent FX failures —
    neither was previously covered by an automated prod test.

    ~$0.20 + ~4 min per test."""

    FA_FIELDS = {
        "fa_payee_name": "Foreign Receipt Test",
        "fa_payee_sunet": "fxtest",
        "fa_payee_affiliation": "stanford_faculty",
        "fa_event_name": "Foreign Receipt Coverage",
        "fa_payment_method": "personal_card",
        "fa_rush_processing": "no",
        "fa_authorized_by": "advisor@stanford.edu",
        "fa_bp_who": "Foreign Receipt Test",
        "fa_bp_what": "Multilingual + FX end-to-end verification",
        "fa_bp_when_from": "2022-01-01",
        "fa_bp_when_to": "2026-12-31",
        "fa_bp_where": "International",
        "fa_bp_why": "Confirm Gemini handles non-English text + Frankfurter FX",
        "fa_foreign_activity_type": "conferences",
    }
    SHOTS_DIR = REPO_ROOT / ".scratch" / "e2e-foreign"

    @classmethod
    def setUpClass(cls):
        cls._playwright = sync_playwright().start()
        cls._browser = cls._playwright.chromium.launch(headless=True)
        cls._token = _gcloud_id_token()
        cls.SHOTS_DIR.mkdir(parents=True, exist_ok=True)

    @classmethod
    def tearDownClass(cls):
        cls._browser.close()
        cls._playwright.stop()

    def _new_context(self):
        return self._browser.new_context(
            viewport={"width": 1400, "height": 1800},
            extra_http_headers={"Authorization": f"Bearer {self._token}"},
        )

    def _submit_one(self, page, file_path: Path, kind: str) -> str:
        page.goto(PROD_URL + "/", wait_until="domcontentloaded")
        for name, value in self.FA_FIELDS.items():
            el = page.query_selector(f'[name="{name}"]')
            if el is None:
                continue
            tag = el.evaluate("e => e.tagName")
            if tag == "SELECT":
                page.select_option(f'[name="{name}"]', value)
            else:
                page.fill(f'[name="{name}"]', value)
        page.set_input_files('[name="file_0"]', str(file_path))
        page.select_option('[name="kind_0"]', kind)
        with page.expect_navigation(wait_until="domcontentloaded",
                                    timeout=60_000):
            page.click('button[type="submit"]')
        return page.url.rstrip("/").split("/")[-1]

    def _fetch_authed(self, path: str) -> bytes:
        req = urllib.request.Request(
            f"{PROD_URL}{path}",
            headers={"Authorization": f"Bearer {self._token}"},
        )
        with urllib.request.urlopen(req, timeout=15) as resp:
            return resp.read()

    def _check_foreign_receipt(self, *, filename: str, region_tag: str,
                                expected_non_usd_currencies: set[str]):
        """Common assertion path: upload → workbench → assert foreign-
        currency line in report.json + corresponding row in
        lines-foreign.csv."""
        ctx = self._new_context()
        page = ctx.new_page()
        receipt = REPO_ROOT / "receipts" / filename
        self.assertTrue(receipt.exists(), f"missing fixture: {receipt}")
        upload_id = self._submit_one(page, receipt, "lodging")
        page.wait_for_url("**/workbench.html", timeout=900_000)
        line_cards = page.query_selector_all("details.line-card")
        self.assertGreaterEqual(len(line_cards), 1,
                                f"{region_tag}: extraction produced no lines")
        page.screenshot(path=str(self.SHOTS_DIR / f"{region_tag}.png"),
                        full_page=True)
        ctx.close()

        # Inspect the actual report to confirm FX landed.
        body = self._fetch_authed(f"/uploads/{upload_id}/reduced/report.json")
        report = json.loads(body.decode())
        lines = report.get("transaction_lines") or []
        self.assertGreaterEqual(len(lines), 1)
        line = lines[0]
        common = line.get("common", {})
        original_currency = (common.get("original_currency") or {}).get("value")
        line_amount_usd = (common.get("line_amount_usd") or {}).get("value")
        original_amount = (common.get("original_amount") or {}).get("value")
        self.assertIn(original_currency, expected_non_usd_currencies,
                      f"{region_tag}: expected one of "
                      f"{expected_non_usd_currencies}, got {original_currency!r}")
        self.assertIsNotNone(line_amount_usd,
                             f"{region_tag}: line_amount_usd not populated "
                             f"— FX conversion may have failed")
        self.assertGreater(line_amount_usd, 0,
                           f"{region_tag}: line_amount_usd should be > 0")
        self.assertIsNotNone(original_amount,
                             f"{region_tag}: original_amount missing")

        # Confirm the line routed to the foreign CSV.
        foreign_csv = self._fetch_authed(
            f"/uploads/{upload_id}/lines-foreign.csv").decode()
        non_header_rows = [
            r for r in foreign_csv.splitlines()[1:] if r.strip()]
        self.assertGreaterEqual(len(non_header_rows), 1,
                                f"{region_tag}: lines-foreign.csv has no data rows")

    def test_french_eur_receipt(self):
        """French ibis Toulouse PDF. Expect EUR, real Frankfurter rate."""
        self._check_foreign_receipt(
            filename="lodging_2023-05-30_ibis-toulouse-universite.pdf",
            region_tag="france-eur",
            expected_non_usd_currencies={"EUR"})

    def test_japanese_jpy_receipt(self):
        """Tokyo TokyuStay JPEG. Expect JPY, kanji + kana in source."""
        self._check_foreign_receipt(
            filename="lodging_2022-10-09_tokyustay-nihombashi.jpeg",
            region_tag="japan-jpy",
            expected_non_usd_currencies={"JPY"})

    def test_indian_inr_receipt(self):
        """Bangalore Velvette JPG. Expect INR (rupee symbol on receipt)."""
        self._check_foreign_receipt(
            filename="lodging_2022-07-12_velvette-bangalore.jpg",
            region_tag="india-inr",
            expected_non_usd_currencies={"INR"})


if __name__ == "__main__":
    unittest.main()
