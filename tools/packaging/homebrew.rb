class SmipMwp < Formula
  desc "SMIP-MWP datapath stack (Rust)"
  homepage "https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust"
  url "https://github.com/rwilliamspbg-ops/SMIP-MWP-Rust/releases/download/v0.0.0/smip-mwp-x86_64-unknown-linux-gnu.tar.gz"
  sha256 "0" # replace with real checksum per release
  license "AGPL-3.0"

  depends_on "rust" => :build

  def install
    bin.install "mohawk-node" if File.exist?("mohawk-node")
  end

  test do
    system "true"
  end

  # Usage: After building release artifacts, update url and sha256 with actual values.
end