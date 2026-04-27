import json
import tempfile
import unittest
from pathlib import Path

import scripts.create_receipt_manifest as manifest_script


class CreateReceiptManifestTests(unittest.TestCase):
    def test_parse_expected_field_requires_key_value(self):
        with self.assertRaises(ValueError):
            manifest_script.parse_expected_field("merchant_name")

    def test_parse_expected_field_parses_key_value(self):
        self.assertEqual(
            manifest_script.parse_expected_field("merchant_name=Corner Store"),
            ("merchant_name", "Corner Store"),
        )

    def test_build_manifest_defaults_ids_and_tags(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            input_path = Path(temp_dir) / "Receipt Scan.png"
            input_path.write_bytes(b"fake")

            class Args:
                input = input_path
                output = Path(temp_dir) / "manifest.json"
                document_id = None
                packet_id = None
                corpus_name = None
                expected_field = ["merchant_name=Corner Store", "total_paid=47.50"]
                source_name = "Local Receipt"
                source_url = ""
                license_name = "private/local"
                tag = None

            manifest = manifest_script.build_manifest(Args())

            self.assertEqual(manifest["corpus_name"], "local_receipt_scan")
            document = manifest["documents"][0]
            self.assertEqual(document["document_id"], "receipt_scan")
            self.assertEqual(document["packet_id"], "receipt_scan")
            self.assertEqual(
                document["expected_fields"],
                {"merchant_name": "Corner Store", "total_paid": "47.50"},
            )
            self.assertEqual(document["tags"], ["local", "receipt"])
            self.assertEqual(document["input_path"], str(input_path.resolve()))

    def test_main_writes_manifest_json(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            input_path = Path(temp_dir) / "receipt.png"
            output_path = Path(temp_dir) / "manifest.json"
            input_path.write_bytes(b"fake")

            original_parse_args = manifest_script.parse_args

            class Args:
                input = input_path
                output = output_path
                document_id = "scanned_receipt"
                packet_id = "scanned_receipt"
                corpus_name = "local_scanned_receipt"
                expected_field = ["transaction_date=2026-04-27"]
                source_name = "Local Receipt"
                source_url = ""
                license_name = "private/local"
                tag = ["local", "background"]

            manifest_script.parse_args = lambda: Args()
            try:
                self.assertEqual(manifest_script.main(), 0)
            finally:
                manifest_script.parse_args = original_parse_args

            payload = json.loads(output_path.read_text())
            self.assertEqual(payload["corpus_name"], "local_scanned_receipt")
            self.assertEqual(payload["documents"][0]["tags"], ["local", "background"])
            self.assertEqual(
                payload["documents"][0]["expected_fields"],
                {"transaction_date": "2026-04-27"},
            )


if __name__ == "__main__":
    unittest.main()
