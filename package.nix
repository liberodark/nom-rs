{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "nix-output-monitor";
  version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;

  postInstall = ''
    ln -s nom $out/bin/nom-build
    ln -s nom $out/bin/nom-shell
  '';

  meta = {
    description = "Processes output of Nix commands to show helpful and pretty information";
    homepage = "https://github.com/liberodark/nom-rs";
    license = lib.licenses.agpl3Plus;
    mainProgram = "nom";
    platforms = lib.platforms.unix;
  };
}
