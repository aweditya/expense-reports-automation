import importlib.util
import unittest
from pathlib import Path


def load_module():
    script_path = (
        Path(__file__).resolve().parent.parent
        / "scripts"
        / "import_receipt_corpus.py"
    )
    spec = importlib.util.spec_from_file_location("import_receipt_corpus", script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


import_receipt_corpus = load_module()


class ImportReceiptCorpusTests(unittest.TestCase):
    def test_build_sroie_expected_fields_extracts_company_date_and_total(self):
        fields = import_receipt_corpus.build_sroie_expected_fields(
            {
                "entities": {
                    "company": "BOOK TA .#",
                    "date": "2018-03-14",
                    "total": "RM 56.00",
                }
            }
        )

        self.assertEqual(
            fields,
            {
                "classification_kind": "receipt",
                "merchant_name": "BOOK TA .#",
                "transaction_date": "2018-03-14",
                "total_paid": "56.00",
            },
        )

    def test_scalar_entity_value_accepts_nested_lists_and_dicts(self):
        value = import_receipt_corpus.scalar_entity_value(
            [{"value": ""}, {"text": " Actual Merchant "}]
        )

        self.assertEqual(value, "Actual Merchant")


if __name__ == "__main__":
    unittest.main()
