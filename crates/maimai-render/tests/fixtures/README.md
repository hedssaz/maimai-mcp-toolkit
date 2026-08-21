# Test font fixture

`DejaVuSans-ASCII.ttf` contains only U+0020–U+007E and is used to keep image tests independent of host fonts.

It was generated from Matplotlib's bundled `DejaVuSans.ttf` with FontTools 4.62.1:

```text
python -m fontTools.subset DejaVuSans.ttf \
  --unicodes=U+0020-007E \
  --output-file=DejaVuSans-ASCII.ttf \
  --layout-features='*' --glyph-names --symbol-cmap --legacy-cmap \
  --notdef-glyph --notdef-outline --recommended-glyphs \
  --name-IDs='*' --name-legacy --name-languages='*'
```

The upstream license is preserved in `LICENSE_DEJAVU`.
