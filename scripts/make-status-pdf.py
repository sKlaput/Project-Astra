from pathlib import Path
import sys


def escape_pdf_text(s: str) -> str:
    return s.replace('\\', r'\\').replace('(', r'\(').replace(')', r'\)')


def build_pdf_from_lines(lines, output_path: Path):
    # Basic page/layout settings (A4-ish points)
    page_width = 595
    page_height = 842
    left_margin = 50
    top_margin = 790
    line_height = 14
    lines_per_page = 52

    pages = []
    for i in range(0, len(lines), lines_per_page):
        page_lines = lines[i:i + lines_per_page]
        y = top_margin
        content_parts = ["BT", "/F1 11 Tf"]
        for line in page_lines:
            safe = escape_pdf_text(line.rstrip())
            content_parts.append(f"1 0 0 1 {left_margin} {y} Tm ({safe}) Tj")
            y -= line_height
        content_parts.append("ET")
        pages.append("\n".join(content_parts).encode("latin-1", errors="replace"))

    objects = []

    def add_obj(data: bytes) -> int:
        objects.append(data)
        return len(objects)

    # 1: Catalog
    # 2: Pages
    # 3..: Page objects and content objects
    catalog_id = add_obj(b"<< /Type /Catalog /Pages 2 0 R >>")

    kids_refs = []
    page_obj_ids = []
    content_obj_ids = []

    # Font object placed later; keep placeholder id known after creation
    font_id_placeholder = 0

    for content in pages:
        content_obj = (
            f"<< /Length {len(content)} >>\nstream\n".encode("ascii")
            + content
            + b"\nendstream"
        )
        content_id = add_obj(content_obj)
        content_obj_ids.append(content_id)

        # Temporary font reference will be patched after font object creation
        page_obj = (
            b"<< /Type /Page /Parent 2 0 R "
            + f"/MediaBox [0 0 {page_width} {page_height}] ".encode("ascii")
            + b"/Resources << /Font << /F1 FONTREF 0 R >> >> "
            + f"/Contents {content_id} 0 R >>".encode("ascii")
        )
        page_id = add_obj(page_obj)
        page_obj_ids.append(page_id)
        kids_refs.append(f"{page_id} 0 R")

    # Font object
    font_id = add_obj(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")

    # Patch page objects with actual font id
    for idx, page_id in enumerate(page_obj_ids):
        patched = objects[page_id - 1].replace(b"FONTREF", str(font_id).encode("ascii"))
        objects[page_id - 1] = patched

    pages_obj = (
        b"<< /Type /Pages /Count "
        + str(len(page_obj_ids)).encode("ascii")
        + b" /Kids [ "
        + " ".join(kids_refs).encode("ascii")
        + b" ] >>"
    )
    objects[1] = pages_obj  # object 2

    # Build xref
    out = bytearray()
    out.extend(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")

    offsets = [0]
    for i, obj in enumerate(objects, start=1):
        offsets.append(len(out))
        out.extend(f"{i} 0 obj\n".encode("ascii"))
        out.extend(obj)
        out.extend(b"\nendobj\n")

    xref_pos = len(out)
    out.extend(f"xref\n0 {len(objects) + 1}\n".encode("ascii"))
    out.extend(b"0000000000 65535 f \n")
    for off in offsets[1:]:
        out.extend(f"{off:010d} 00000 n \n".encode("ascii"))

    out.extend(
        (
            "trailer\n"
            f"<< /Size {len(objects) + 1} /Root {catalog_id} 0 R >>\n"
            "startxref\n"
            f"{xref_pos}\n"
            "%%EOF\n"
        ).encode("ascii")
    )

    output_path.write_bytes(out)


def main():
    src = Path("docs/status-summary-2026-04-05.md")
    dst = Path("docs/status-summary-2026-04-05.pdf")

    if len(sys.argv) >= 2:
        src = Path(sys.argv[1])
    if len(sys.argv) >= 3:
        dst = Path(sys.argv[2])

    text = src.read_text(encoding="utf-8")
    lines = text.splitlines()
    build_pdf_from_lines(lines, dst)
    print(f"Wrote {dst}")


if __name__ == "__main__":
    main()
