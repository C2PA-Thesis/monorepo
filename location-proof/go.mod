module github.com/C2PA-Thesis/monorepo/location-proof

go 1.23

require (
	github.com/consensys/gnark v0.9.1
	github.com/consensys/gnark-crypto v0.12.2-0.20240215234832-d72fcb379d3e
	github.com/tumberger/zk-Location v0.0.0-20260903012756-cc8f1c745156
	github.com/uber/h3-go/v4 v4.1.0
)

require (
	github.com/bits-and-blooms/bitset v1.13.0 // indirect
	github.com/blang/semver/v4 v4.0.0 // indirect
	github.com/consensys/bavard v0.1.13 // indirect
	github.com/davecgh/go-spew v1.1.1 // indirect
	github.com/fxamacker/cbor/v2 v2.6.0 // indirect
	github.com/google/pprof v0.0.0-20240319011627-a57c5dfe54fd // indirect
	github.com/ingonyama-zk/icicle v0.1.0 // indirect
	github.com/ingonyama-zk/iciclegnark v0.1.1 // indirect
	github.com/mattn/go-colorable v0.1.13 // indirect
	github.com/mattn/go-isatty v0.0.20 // indirect
	github.com/mmcloughlin/addchain v0.4.0 // indirect
	github.com/pmezard/go-difflib v1.0.0 // indirect
	github.com/rs/zerolog v1.32.0 // indirect
	github.com/stretchr/testify v1.9.0 // indirect
	github.com/x448/float16 v0.8.4 // indirect
	golang.org/x/crypto v0.21.0 // indirect
	golang.org/x/sync v0.6.0 // indirect
	golang.org/x/sys v0.18.0 // indirect
	gopkg.in/yaml.v3 v3.0.1 // indirect
	rsc.io/tmplfunc v0.0.3 // indirect
)

// The ZKLP fork has no license, so its code is referenced here rather than copied.
// gnark is replaced the same way the fork's own go.mod does; Go ignores that
// replacement when the fork is used as a dependency.
replace (
	github.com/consensys/gnark => github.com/winderica/gnark v0.0.0-20240319143525-e30c94cd2e7e
	github.com/tumberger/zk-Location => github.com/C2PA-Thesis/zk-Location v0.0.0-20260903012756-cc8f1c745156
)
