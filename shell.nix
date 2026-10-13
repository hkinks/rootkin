{ pkgs ? import <nixpkgs> { } }:
let
  runtimeLibs = with pkgs; [
    libGL
    vulkan-loader
    libxkbcommon
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
  ];
in
pkgs.mkShell {
  packages = with pkgs; [ cargo rustc clippy rustfmt rust-analyzer pkg-config ];
  buildInputs = runtimeLibs;
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
}
