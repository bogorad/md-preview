# Builds the working tree of this repository.
#
# There is deliberately no fetchFromGitHub and no vendor hash: the version is
# read from Cargo.toml and the dependency set from the committed Cargo.lock, so
# a version bump is a normal commit with nothing to re-hash by hand.

{
  lib,
  rustPlatform,
  pkg-config,
  wrapGAppsHook3,
  copyDesktopItems,
  makeDesktopItem,
  webkitgtk_4_1,
  gtk3,
  libsoup_3,
  glib-networking,
}:

rustPlatform.buildRustPackage {
  pname = "md-preview";
  version = (lib.importTOML ../Cargo.toml).package.version;

  # Only what the crate actually compiles from. Using the whole tree would mean
  # that a README, a workflow or a docs commit changes the source hash and
  # forces every consumer into a full rebuild of the Rust dependency graph.
  #
  # The list is exhaustive: build.rs, everything include_str!/include_bytes!'d
  # out of assets/, and Cargo.toml, which env!("CARGO_PKG_VERSION") reads at
  # compile time. The tests are inline in src/main.rs and touch no repository
  # files at runtime.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../build.rs
      ../src
      ../assets
    ];
  };

  # Every dependency resolves to crates.io, so the committed lock file is
  # enough and no vendor hash has to be tracked alongside it.
  cargoLock.lockFile = ../Cargo.lock;

  # No update checking of any kind.
  #
  # Upstream injects assets/enhance/update-check.js into every rendered page and
  # calls installUpdateCheck() at the bottom of it. That function is the sole
  # trigger for the whole feature: it wires the toolbar button, defines the
  # fetch closure and schedules a call to api.github.com after first paint.
  # Overriding it with a no-op after the file's own IIFE has run therefore
  # removes every network request while leaving the call site valid.
  #
  # It has to be appended rather than replace the file, because the page
  # assertions in src/main.rs check for literal strings that live inside it
  # ("update-check-result:available" among them).
  #
  # The other path, check_github_updates(), shells out to curl, but its only
  # sender is send_macos_menu_event() and it is therefore already unreachable
  # on Linux.
  postPatch = ''
    echo 'window.__mdPreviewInstallUpdateCheck = function () {};' \
      >> assets/enhance/update-check.js
  '';

  nativeBuildInputs = [
    # wry, gtk-sys and soup3-sys all locate their C libraries through pkg-config.
    pkg-config
    # Bakes GIO_EXTRA_MODULES, GSETTINGS_SCHEMA_DIR and GDK_PIXBUF_MODULE_FILE
    # into the wrapper. Without it the webview starts, but glib has no TLS
    # backend and every https:// fetch fails silently.
    wrapGAppsHook3
    copyDesktopItems
  ];

  buildInputs = [
    # wry 0.50 binds the webkit2gtk *4.1* ABI, which is GTK 3 and libsoup 3.
    # webkitgtk 6.0 is the GTK 4 build and does not satisfy it.
    webkitgtk_4_1
    gtk3
    libsoup_3
    # Present for wrapGAppsHook3 to discover, not to link against.
    glib-networking
  ];

  desktopItems = [
    (makeDesktopItem {
      name = "md-preview";
      desktopName = "MD Preview";
      genericName = "Markdown Viewer";
      comment = "Preview Markdown with live reload, KaTeX and Mermaid";
      exec = "md-preview %f";
      icon = "md-preview";
      terminal = false;
      categories = [
        "Office"
        "Viewer"
      ];
      # No MimeType is declared on purpose. It would not change any default
      # application by itself, but it does place an entry in the MIME index,
      # and this package leaves file associations alone entirely. Adding
      # `mimeTypes = [ "text/markdown" "text/x-markdown" ];` here is all it
      # takes to appear under "Open With" if that is ever wanted.
      keywords = [
        "Markdown"
        "md"
        "preview"
      ];
    })
  ];

  # Everything under assets/ (highlight.js, KaTeX, Mermaid, the icon) is
  # include_str!/include_bytes!'d into the binary at compile time, so the icon
  # is installed only so that desktop environments can display it.
  postInstall = ''
    install -Dm644 assets/icon_1024.png \
      $out/share/icons/hicolor/1024x1024/apps/md-preview.png
  '';

  meta = {
    description = "Native Markdown previewer with live reload, KaTeX and Mermaid";
    homepage = "https://github.com/bogorad/md-preview";
    license = lib.licenses.mit;
    mainProgram = "md-preview";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
}
