# Public demo fixture

The demo capture uses latitude `-34.5478` and longitude `-58.4462`, a public
test point near Universidad Torcuato Di Tella in Buenos Aires. At H3 resolution
7, that point maps to cell `87c2e3020ffffff` and the canonical face/IJK tuple in
`region-utdt-res7.json`.

These committed coordinates test that the published assertion and proofs do
not disclose the private witness. They are not secret in this source tree and
must not be presented as a real privacy-sensitive capture.

`pipeline/setup.sh` creates `fixtures/generated/` with a demo input image and a
separate P-256 device key pair. Both key files are ignored. The C2PA sample
signing key has a different purpose and is never used as the device key.
