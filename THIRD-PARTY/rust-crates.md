# Rustクレートの一覧

`tools/third-party.py` が作るファイルです。手では直さないでください。

Cargo.lock で固定した版のうち、Linuxのx86-64(glibcとmusl)で使うものを載せています。実行ファイルに入るものが67個、ビルドのときだけ使うものが19個です。

| クレート | 版 | ライセンス | 使われ方 | ライセンス文 |
| --- | --- | --- | --- | --- |
| appendlist | 1.4.0 | MIT | 実行ファイルに入る | [licenses/appendlist-1.4.0/](licenses/appendlist-1.4.0/) |
| approx | 0.4.0 | Apache-2.0 | 実行ファイルに入る | [licenses/approx-0.4.0/](licenses/approx-0.4.0/) |
| atomic_float | 1.1.0 | Apache-2.0 OR MIT OR Unlicense | 実行ファイルに入る | [licenses/atomic_float-1.1.0/](licenses/atomic_float-1.1.0/) |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | ビルドのときだけ | [licenses/autocfg-1.5.1/](licenses/autocfg-1.5.1/) |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/bitflags-2.13.2/](licenses/bitflags-2.13.2/) |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/block-buffer-0.10.4/](licenses/block-buffer-0.10.4/) |
| calloop | 0.14.5 | MIT | 実行ファイルに入る | [licenses/calloop-0.14.5/](licenses/calloop-0.14.5/) |
| calloop-wayland-source | 0.4.1 | MIT | 実行ファイルに入る | [licenses/calloop-wayland-source-0.4.1/](licenses/calloop-wayland-source-0.4.1/) |
| cc | 1.6.0 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/cc-1.6.0/](licenses/cc-1.6.0/) |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/cfg-if-1.0.5/](licenses/cfg-if-1.0.5/) |
| cgmath | 0.18.0 | Apache-2.0 | 実行ファイルに入る | [licenses/cgmath-0.18.0/](licenses/cgmath-0.18.0/) |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/cpufeatures-0.2.17/](licenses/cpufeatures-0.2.17/) |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/crypto-common-0.1.7/](licenses/crypto-common-0.1.7/) |
| cursor-icon | 1.2.0 | MIT OR Apache-2.0 OR Zlib | 実行ファイルに入る | [licenses/cursor-icon-1.2.0/](licenses/cursor-icon-1.2.0/) |
| digest | 0.10.7 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/digest-0.10.7/](licenses/digest-0.10.7/) |
| downcast-rs | 1.2.1 | MIT/Apache-2.0 | 実行ファイルに入る | [licenses/downcast-rs-1.2.1/](licenses/downcast-rs-1.2.1/) |
| drm-fourcc | 2.2.0 | MIT | 実行ファイルに入る | 配布物に入っていない |
| equivalent | 1.0.2 | Apache-2.0 OR MIT | 実行ファイルに入る | [licenses/equivalent-1.0.2/](licenses/equivalent-1.0.2/) |
| errno | 0.3.14 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/errno-0.3.14/](licenses/errno-0.3.14/) |
| fastrand | 2.5.0 | Apache-2.0 OR MIT | 実行ファイルに入る | [licenses/fastrand-2.5.0/](licenses/fastrand-2.5.0/) |
| find-msvc-tools | 0.1.14 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/find-msvc-tools-0.1.14/](licenses/find-msvc-tools-0.1.14/) |
| generic-array | 0.14.7 | MIT | 実行ファイルに入る | [licenses/generic-array-0.14.7/](licenses/generic-array-0.14.7/) |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/getrandom-0.3.4/](licenses/getrandom-0.3.4/) |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/getrandom-0.4.3/](licenses/getrandom-0.4.3/) |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/hashbrown-0.17.1/](licenses/hashbrown-0.17.1/) |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | 実行ファイルに入る | [licenses/indexmap-2.14.2/](licenses/indexmap-2.14.2/) |
| lazy_static | 1.5.1 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/lazy_static-1.5.1/](licenses/lazy_static-1.5.1/) |
| libc | 0.2.190 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/libc-0.2.190/](licenses/libc-0.2.190/) |
| linux-raw-sys | 0.12.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 実行ファイルに入る | [licenses/linux-raw-sys-0.12.1/](licenses/linux-raw-sys-0.12.1/) |
| log | 0.4.34 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/log-0.4.34/](licenses/log-0.4.34/) |
| memchr | 2.8.3 | Unlicense OR MIT | ビルドのときだけ | [licenses/memchr-2.8.3/](licenses/memchr-2.8.3/) |
| memmap2 | 0.9.11 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/memmap2-0.9.11/](licenses/memmap2-0.9.11/) |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/num-traits-0.2.19/](licenses/num-traits-0.2.19/) |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/once_cell-1.21.4/](licenses/once_cell-1.21.4/) |
| paste | 1.0.15 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/paste-1.0.15/](licenses/paste-1.0.15/) |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | 実行ファイルに入る | [licenses/pin-project-lite-0.2.17/](licenses/pin-project-lite-0.2.17/) |
| pixman | 0.2.1 | MIT | 実行ファイルに入る | 配布物に入っていない |
| pixman-sys | 0.1.0 | MIT | 実行ファイルに入る | 配布物に入っていない |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/pkg-config-0.3.34/](licenses/pkg-config-0.3.34/) |
| polling | 3.11.0 | Apache-2.0 OR MIT | 実行ファイルに入る | [licenses/polling-3.11.0/](licenses/polling-3.11.0/) |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/ppv-lite86-0.2.21/](licenses/ppv-lite86-0.2.21/) |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/proc-macro2-1.0.107/](licenses/proc-macro2-1.0.107/) |
| profiling | 1.0.18 | MIT OR Apache-2.0 | 実行ファイルに入る | 配布物に入っていない |
| profiling-procmacros | 1.0.18 | MIT OR Apache-2.0 | ビルドのときだけ | 配布物に入っていない |
| quick-xml | 0.41.0 | MIT | ビルドのときだけ | [licenses/quick-xml-0.41.0/](licenses/quick-xml-0.41.0/) |
| quote | 1.0.47 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/quote-1.0.47/](licenses/quote-1.0.47/) |
| rand | 0.9.5 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/rand-0.9.5/](licenses/rand-0.9.5/) |
| rand_chacha | 0.9.0 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/rand_chacha-0.9.0/](licenses/rand_chacha-0.9.0/) |
| rand_core | 0.9.5 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/rand_core-0.9.5/](licenses/rand_core-0.9.5/) |
| rustix | 1.1.5 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | 実行ファイルに入る | [licenses/rustix-1.1.5/](licenses/rustix-1.1.5/) |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/sha2-0.10.9/](licenses/sha2-0.10.9/) |
| sharded-slab | 0.1.7 | MIT | 実行ファイルに入る | [licenses/sharded-slab-0.1.7/](licenses/sharded-slab-0.1.7/) |
| shlex | 2.0.1 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/shlex-2.0.1/](licenses/shlex-2.0.1/) |
| smallvec | 1.16.2 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/smallvec-1.16.2/](licenses/smallvec-1.16.2/) |
| smithay | 0.7.0 | MIT | 実行ファイルに入る | [licenses/smithay-0.7.0/](licenses/smithay-0.7.0/) |
| smithay-client-toolkit | 0.21.1 | MIT | 実行ファイルに入る | [licenses/smithay-client-toolkit-0.21.1/](licenses/smithay-client-toolkit-0.21.1/) |
| syn | 2.0.119 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/syn-2.0.119/](licenses/syn-2.0.119/) |
| syn | 3.0.6 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/syn-3.0.6/](licenses/syn-3.0.6/) |
| tempfile | 3.27.0 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/tempfile-3.27.0/](licenses/tempfile-3.27.0/) |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/thiserror-1.0.69/](licenses/thiserror-1.0.69/) |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/thiserror-2.0.21/](licenses/thiserror-2.0.21/) |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/thiserror-impl-1.0.69/](licenses/thiserror-impl-1.0.69/) |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | ビルドのときだけ | [licenses/thiserror-impl-2.0.21/](licenses/thiserror-impl-2.0.21/) |
| thread_local | 1.1.10 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/thread_local-1.1.10/](licenses/thread_local-1.1.10/) |
| tracing | 0.1.44 | MIT | 実行ファイルに入る | [licenses/tracing-0.1.44/](licenses/tracing-0.1.44/) |
| tracing-attributes | 0.1.31 | MIT | ビルドのときだけ | [licenses/tracing-attributes-0.1.31/](licenses/tracing-attributes-0.1.31/) |
| tracing-core | 0.1.36 | MIT | 実行ファイルに入る | [licenses/tracing-core-0.1.36/](licenses/tracing-core-0.1.36/) |
| tracing-subscriber | 0.3.23 | MIT | 実行ファイルに入る | [licenses/tracing-subscriber-0.3.23/](licenses/tracing-subscriber-0.3.23/) |
| typenum | 1.20.1 | MIT OR Apache-2.0 | 実行ファイルに入る | [licenses/typenum-1.20.1/](licenses/typenum-1.20.1/) |
| unicode-ident | 1.0.26 | (MIT OR Apache-2.0) AND Unicode-3.0 | ビルドのときだけ | [licenses/unicode-ident-1.0.26/](licenses/unicode-ident-1.0.26/) |
| version_check | 0.9.5 | MIT/Apache-2.0 | ビルドのときだけ | [licenses/version_check-0.9.5/](licenses/version_check-0.9.5/) |
| wayland-backend | 0.3.17 | MIT | 実行ファイルに入る | [licenses/wayland-backend-0.3.17/](licenses/wayland-backend-0.3.17/) |
| wayland-client | 0.31.15 | MIT | 実行ファイルに入る | [licenses/wayland-client-0.31.15/](licenses/wayland-client-0.31.15/) |
| wayland-csd-frame | 0.3.0 | MIT | 実行ファイルに入る | [licenses/wayland-csd-frame-0.3.0/](licenses/wayland-csd-frame-0.3.0/) |
| wayland-cursor | 0.31.14 | MIT | 実行ファイルに入る | [licenses/wayland-cursor-0.31.14/](licenses/wayland-cursor-0.31.14/) |
| wayland-protocols | 0.32.13 | MIT | 実行ファイルに入る | [licenses/wayland-protocols-0.32.13/](licenses/wayland-protocols-0.32.13/) |
| wayland-protocols-experimental | 20251230.0.3 | MIT | 実行ファイルに入る | [licenses/wayland-protocols-experimental-20251230.0.3/](licenses/wayland-protocols-experimental-20251230.0.3/) |
| wayland-protocols-misc | 0.3.12 | MIT | 実行ファイルに入る | [licenses/wayland-protocols-misc-0.3.12/](licenses/wayland-protocols-misc-0.3.12/) |
| wayland-protocols-wlr | 0.3.12 | MIT | 実行ファイルに入る | [licenses/wayland-protocols-wlr-0.3.12/](licenses/wayland-protocols-wlr-0.3.12/) |
| wayland-scanner | 0.31.11 | MIT | ビルドのときだけ | [licenses/wayland-scanner-0.31.11/](licenses/wayland-scanner-0.31.11/) |
| wayland-server | 0.31.14 | MIT | 実行ファイルに入る | [licenses/wayland-server-0.31.14/](licenses/wayland-server-0.31.14/) |
| wayland-sys | 0.31.11 | MIT | 実行ファイルに入る | [licenses/wayland-sys-0.31.11/](licenses/wayland-sys-0.31.11/) |
| xcursor | 0.3.11 | MIT | 実行ファイルに入る | [licenses/xcursor-0.3.11/](licenses/xcursor-0.3.11/) |
| xkbcommon | 0.8.0 | MIT | 実行ファイルに入る | [licenses/xkbcommon-0.8.0/](licenses/xkbcommon-0.8.0/) |
| xkeysym | 0.2.1 | MIT OR Apache-2.0 OR Zlib | 実行ファイルに入る | [licenses/xkeysym-0.2.1/](licenses/xkeysym-0.2.1/) |
| zerocopy | 0.8.59 | BSD-2-Clause OR Apache-2.0 OR MIT | 実行ファイルに入る | [licenses/zerocopy-0.8.59/](licenses/zerocopy-0.8.59/) |
