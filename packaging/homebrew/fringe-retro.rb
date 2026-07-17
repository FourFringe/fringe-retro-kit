# typed: true
# frozen_string_literal: true

# Homebrew formula for Fringe Retro Kit.
#
# This is the canonical copy, versioned alongside the code. It is mirrored to the Homebrew
# tap repository (FourFringe/homebrew-tap) as `Formula/fringe-retro.rb`, from where users
# install it:
#
#   brew install FourFringe/tap/fringe-retro
#
# It installs the pre-built macOS binary from the matching GitHub release (no compilation).
class FringeRetro < Formula
  desc "Inspect, edit, back up, and browse world maps from classic game saves"
  homepage "https://github.com/FourFringe/fringe-retro-kit"
  version "0.2.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/FourFringe/fringe-retro-kit/releases/download/v#{version}/fringe-retro-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "0723899794e04862d5a8d4735032be4179d486fcc654eaad656fc6acf296dc1b"
    end
    on_intel do
      url "https://github.com/FourFringe/fringe-retro-kit/releases/download/v#{version}/fringe-retro-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "605e1a2c6c57dc61462843c194c27f5b76bc2f8627bc4783c7a03cfe008e3033"
    end
  end

  def install
    bin.install "fringe-retro", "fringe-retro-map"
    # The release tarball also bundles the docs.
    doc.install "README.md", "COMMANDS.md" if File.exist?("README.md")
  end

  test do
    assert_match "fringe-retro #{version}", shell_output("#{bin}/fringe-retro --version")
    assert_match "fringe-retro-map #{version}", shell_output("#{bin}/fringe-retro-map --version")
  end
end
