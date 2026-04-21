import importlib.util
import io
import unittest
from pathlib import Path
from types import SimpleNamespace


def load_module():
    script_path = (
        Path(__file__).resolve().parent.parent
        / "scripts"
        / "transcribe_with_document_ai.py"
    )
    spec = importlib.util.spec_from_file_location("transcribe_with_document_ai", script_path)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


transcribe = load_module()


def make_text_anchor(*segments):
    return SimpleNamespace(
        text_segments=[
            SimpleNamespace(start_index=start, end_index=end) for start, end in segments
        ]
    )


def make_layout(anchor, vertices):
    return SimpleNamespace(
        text_anchor=anchor,
        bounding_poly=SimpleNamespace(
            vertices=[SimpleNamespace(x=x, y=y) for x, y in vertices],
            normalized_vertices=[],
        ),
    )


class TranscribeWithDocumentAiTests(unittest.TestCase):
    def test_extract_text_from_anchor_concatenates_segments(self):
        text = "BOOK TALK 25/12/2018 TOTAL RM 9.00"
        anchor = make_text_anchor((0, 9), (10, 20))

        extracted = transcribe.extract_text_from_anchor(text, anchor)

        self.assertEqual(extracted, "BOOK TALK25/12/2018")

    def test_layout_bbox_normalizes_vertex_coordinates(self):
        page = SimpleNamespace(
            page_number=1,
            dimension=SimpleNamespace(width=1000, height=2000),
        )
        layout = make_layout(make_text_anchor((0, 4)), [(100, 200), (700, 200), (700, 400), (100, 400)])

        bbox = transcribe.layout_bbox(layout, page)

        self.assertEqual(
            bbox,
            {"left": 0.1, "top": 0.1, "width": 0.6, "height": 0.1},
        )

    def test_page_regions_collects_lines_and_table_cells(self):
        document_text = "BOOK TALK\nTOTAL RM 9.00"
        page = SimpleNamespace(
            page_number=1,
            dimension=SimpleNamespace(width=1000, height=1000),
            blocks=[],
            lines=[
                SimpleNamespace(
                    layout=make_layout(make_text_anchor((0, 9)), [(0, 0), (500, 0), (500, 50), (0, 50)])
                ),
                SimpleNamespace(
                    layout=make_layout(make_text_anchor((10, 24)), [(0, 60), (500, 60), (500, 120), (0, 120)])
                ),
            ],
            tokens=[],
            tables=[
                SimpleNamespace(
                    layout=make_layout(make_text_anchor((10, 24)), [(0, 60), (500, 60), (500, 120), (0, 120)]),
                    body_rows=[
                        SimpleNamespace(
                            cells=[
                                SimpleNamespace(
                                    layout=make_layout(
                                        make_text_anchor((10, 15)),
                                        [(0, 60), (250, 60), (250, 120), (0, 120)],
                                    )
                                ),
                                SimpleNamespace(
                                    layout=make_layout(
                                        make_text_anchor((16, 24)),
                                        [(260, 60), (500, 60), (500, 120), (260, 120)],
                                    )
                                ),
                            ]
                        )
                    ],
                )
            ],
        )

        regions = transcribe.page_regions(page, document_text)

        self.assertEqual(regions[0]["kind"], "line")
        self.assertEqual(regions[0]["text"], "BOOK TALK")
        self.assertTrue(any(region["kind"] == "table_cell" for region in regions))

    def test_normalize_document_marks_geometry_available_when_regions_have_boxes(self):
        document = SimpleNamespace(
            text="BOOK TALK\nTOTAL RM 9.00",
            pages=[
                SimpleNamespace(
                    page_number=1,
                    dimension=SimpleNamespace(width=1000, height=1000),
                    layout=SimpleNamespace(text_anchor=make_text_anchor((0, 24))),
                    blocks=[],
                    lines=[
                        SimpleNamespace(
                            layout=make_layout(
                                make_text_anchor((0, 9)),
                                [(0, 0), (500, 0), (500, 50), (0, 50)],
                            )
                        )
                    ],
                    tokens=[],
                    tables=[],
                )
            ],
        )

        payload = transcribe.normalize_document(
            document,
            filename="receipt.png",
            source_path=Path("receipt.png"),
            pass_id="docai_primary",
            pass_kind="primary",
            preprocess_variant="original",
            processor_name="processors/ocr-demo",
        )

        self.assertEqual(payload["metadata"]["geometry_source"], "document_ai")
        self.assertTrue(payload["metadata"]["geometry_available"])
        self.assertEqual(payload["pages"][0]["regions"][0]["text"], "BOOK TALK")

    def test_preprocess_document_bytes_binarized_renders_png(self):
        from PIL import Image

        image = Image.new("RGB", (8, 8), color=(220, 220, 220))
        image.putpixel((4, 4), (20, 20, 20))
        buffer = io.BytesIO()
        image.save(buffer, format="PNG")

        processed_bytes, processed_mime = transcribe.preprocess_document_bytes(
            buffer.getvalue(), "image/png", "binarized"
        )

        self.assertEqual(processed_mime, "image/png")
        processed = Image.open(io.BytesIO(processed_bytes))
        self.assertEqual(processed.size, (8, 8))


if __name__ == "__main__":
    unittest.main()
