# Security

This is research software running on a test network. It has not been audited. Do not use it to protect
real funds.

## Reporting

Please report vulnerabilities privately through GitHub's security advisories for this repository rather
than in a public issue. Include the affected component, a description, and steps to reproduce.

## Known limitations

- The packet format uses BLAKE3 as a stream cipher and a per-hop stream layer instead of a wide-block
  cipher; integrity is checked end to end at the last hop.
- Each node's replay set grows for the node's lifetime; production deployments rotate node keys.
- The rate limiter's token value travels in the clear to the gateway: an on-path observer can exhaust
  one message's continuation budget, but cannot mint budget.
- The exit prover sees which spends it pairs; the mixnet hides who sent them, and the spends themselves
  are zero knowledge.
- The Lean model abstracts the proof system and leanSPHINCS as sound; that the model matches the
  deployed bytecode is checked by differential testing, not proven.
- The proof systems (Spartan-WHIR, leanVM, Binius64) are research implementations.
