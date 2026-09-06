# Maintainer: Bayraktar1392 <bayraktar1392@proton.me>
pkgname=clipper
pkgver=3.0.3
pkgrel=1
pkgdesc="Minimal native GTK4/libadwaita downloader for Twitch Clips, YouTube and TikTok videos with automatic URL queueing"
arch=('x86_64' 'aarch64')
url="https://github.com/Bayraktar1392/Clipper"
license=('MIT')
depends=(
    'gtk4'
    'libadwaita'
    'gdk-pixbuf2'
    'pango'
    'glib2'
    'cairo'
    'graphene'
    'hicolor-icon-theme'
    'yt-dlp'
    'xdg-utils'
)
makedepends=(
    'rust'
    'git'
    'meson'
    'pkgconf'
)
optdepends=(
    'libnotify: desktop notifications'
    'libcanberra: completion sound support'
    'pulseaudio: completion sound support'
    'pipewire: completion sound support'
)
provides=('clipper')
conflicts=('clipper-git')
source=("${pkgname}-${pkgver}.tar.gz::https://github.com/Bayraktar1392/Clipper/archive/refs/tags/v${pkgver}.tar.gz")
sha256sums=('SKIP')

_srcdir="Clipper-${pkgver}"

prepare() {
    cd "${_srcdir}"
    cargo fetch --target "$(rustc -vV | sed -n 's/^host: //p')"
}

build() {
    cd "${_srcdir}"
    export RUSTUP_TOOLCHAIN=stable
    export CARGO_TARGET_DIR=target
    cargo build --frozen --release --all-features
}

check() {
    cd "${_srcdir}"
    export RUSTUP_TOOLCHAIN=stable
    cargo test --frozen --all-features
}

package() {
    cd "${_srcdir}"
    
    # Install binary
    install -Dm755 "target/release/${pkgname}" "${pkgdir}/usr/bin/${pkgname}"
    
    # Install desktop file
    install -Dm644 "assets/clipper.desktop" "${pkgdir}/usr/share/applications/clipper.desktop"
    
    # Install icon
    install -Dm644 "assets/icons/hicolor/scalable/apps/clipper.svg" \
        "${pkgdir}/usr/share/icons/hicolor/scalable/apps/clipper.svg"
    
    # Install license
    install -Dm644 "LICENSE" "${pkgdir}/usr/share/licenses/${pkgname}/LICENSE"
    
    # Install README
    install -Dm644 "README.md" "${pkgdir}/usr/share/doc/${pkgname}/README.md"
}