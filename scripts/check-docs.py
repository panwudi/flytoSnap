#!/usr/bin/env python3
"""Check documentation links and locale-specific screenshot references."""
from pathlib import Path
import re

root = Path(__file__).resolve().parent.parent
languages = ["zh-CN", "zh-TW", "en", "ja", "ru", "fr", "es", "pt", "de", "ko"]
for language in languages:
    name = f"README.{language}.md"
    text = (root / name).read_text()
    for target in re.findall(r"\]\(([^)]+)\)", text):
        if target.startswith(("https://", "http://", "#")):
            continue
        path = target.split("#", 1)[0]
        assert (root / path).is_file(), f"Broken link in {name}: {target}"
        if path.startswith("docs/images/"):
            assert path.endswith(f"-{language}.png"), f"Wrong screenshot language in {name}: {path}"
    assert "releases/latest" in text, f"Missing download link in {name}"
    assert f"ocr-{language}.png" in text, f"Missing localized OCR screenshot in {name}"
    for readme in [f"README.{code}.md" for code in languages]:
        assert readme in text, f"Missing language navigation in {name}"
overview = (root / "README.md").read_text()
for language in languages:
    assert f"README.{language}.md" in overview
    assert f"screenshot-{language}.png" in overview
print("Documentation check passed: language navigation, downloads, local links, and matching screenshots.")
