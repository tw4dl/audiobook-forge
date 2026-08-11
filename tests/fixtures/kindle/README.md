# Kindle test fixtures

The generated files contain only the original text in `source.html` and, for
`with-cover.azw3`, the original art in `cover.svg`. They were generated with
Calibre 8.7.0 on 2026-08-10:

```text
ebook-convert source.html structured.mobi --dont-compress --mobi-file-type old --title "Kindle Fixture" --authors "Example Author" --language en --level1-toc '//h:h1' --level2-toc '//h:h2'
ebook-convert source.html structured-palmdoc.mobi --mobi-file-type old --title "Kindle Fixture" --authors "Example Author" --language en --level1-toc '//h:h1' --level2-toc '//h:h2'
ebook-convert source.html structured.azw3 --dont-compress --title "Kindle Fixture" --authors "Example Author" --language en --level1-toc '//h:h1' --level2-toc '//h:h2'
sips -s format png cover.svg --out cover.png
ebook-convert source.html with-cover.azw3 --dont-compress --cover cover.png --title "Kindle Fixture" --authors "Example Author" --language en --level1-toc '//h:h1' --level2-toc '//h:h2'
```

The fixture source and generated files are available under this repository's
Apache-2.0 license. Calibre is used only to generate test data. It is not a
runtime dependency.
