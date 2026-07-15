Packaging examples and helper notes for producing `.deb`, `.rpm`, and Homebrew formulas.

This folder contains example scripts and pointers. They are intentionally minimal; adapt to your release process.

- `package_deb.sh` - example script that uses `cargo deb` to produce a .deb package (requires `cargo-deb`).

Install `cargo-deb`:

```sh
cargo install cargo-deb
```

Example usage (from repo root):

```sh
tools/packaging/package_deb.sh
```

For RPM or Homebrew packaging, consult upstream documentation and adapt CI steps to produce artifacts in the `dist/` directory.

Homebrew formula template
-------------------------

We include a minimal Homebrew formula template at `tools/packaging/homebrew.rb`. After publishing release tarballs, update the `url` and `sha256` in the formula and publish it in a tap or in the core/brewrepo as appropriate.

Example: generate a tarball for Homebrew and compute sha256

```sh
mkdir -p dist/homebrew
tar -C target/x86_64-unknown-linux-gnu/release -czf dist/homebrew/smip-mwp-x86_64-unknown-linux-gnu.tar.gz <binary-files>
sha256sum dist/homebrew/smip-mwp-x86_64-unknown-linux-gnu.tar.gz
```

Then update `tools/packaging/homebrew.rb` with the `url` and `sha256` and publish.