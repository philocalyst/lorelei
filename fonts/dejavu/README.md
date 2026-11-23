# DejaVu 2.37

`DejaVuSans.ttf`, `DejaVuSans-Bold.ttf` and `DejaVuSansMono.ttf` from the official release
`dejavu-fonts-ttf-2.37.tar.bz2` (https://github.com/dejavu-fonts/dejavu-fonts/releases/tag/version_2_37,
sha256 `fa9ca4d13871dd122f61258a80d01751d603b4d3ee14095d65453b4e846e17d7`, the archive nixpkgs' `dejavu_fonts`
installs), unmodified; `LICENSE` is the release's (Bitstream Vera / Arev terms, DejaVu changes public domain).
`DejaVuSans.ttf` is byte-identical to zgx's `comparisons/fonts/DejaVuSans.ttf` (`zgx_workload::FONT_BYTES`).

Tests, benches and the comparison apps read these by default (`concat!(env!("CARGO_MANIFEST_DIR"),
"/../../fonts/dejavu")`); `BOUGH_DEJAVU_DIR` (and the per-face variables each site documents) still override.

| file | sha256 |
|---|---|
| DejaVuSans.ttf | 7da195a74c55bef988d0d48f9508bd5d849425c1770dba5d7bfc6ce9ed848954 |
| DejaVuSans-Bold.ttf | e6476c1b80502924294eed40894c5b18e06c181444ca953e5334262df9c27724 |
| DejaVuSansMono.ttf | b4a6c3e4faab8773f4ff761d56451646409f29abedd68f05d38c2df667d3c582 |
