# Whitepaper

`topi.tex` is the project's whitepaper: the design of the shielded pool, the proof system, the mixnet,
rate limiting, post-quantum authorization, the ERC-8421 alternative mempool for settles and the Lean
model, with measurements from the test network.

```bash
tectonic topi.tex            # or: latexmk -xelatex topi.tex
```

It needs XeTeX (for `fontspec` and the Libertinus fonts) and TikZ. [Tectonic](https://tectonic-typesetting.github.io)
fetches everything it needs on the first run. The website serves the built PDF as `/whitepaper.pdf`;
after changing the paper, copy `topi.pdf` to `website/public/whitepaper.pdf`.
