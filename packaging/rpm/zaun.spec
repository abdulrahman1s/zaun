# Zaun RPM (PKG-003, RELEASE_STRATEGY §2), for Fedora and COPR.
#
# From source (COPR): Source0 is the GitHub tag archive, Source1 the vendored crates that
# the release workflow attaches (zaun-<version>-vendor.tar.zst), so the build is
# offline. The shim is linked statically against glibc (+crt-static, glibc-static), because
# Fedora's Rust has no musl standard library; it uses no NSS and behaves the same.
#
# From the release binaries (CI, `--with prebuilt`): Source2 is the release tarball, which
# is repackaged without rebuilding (its binary is built for /usr/libexec/zaun).
#
#   rpmbuild -ba packaging/rpm/zaun.spec                       # sources in SOURCES/
#   rpmbuild -bb --with prebuilt packaging/rpm/zaun.spec

%bcond prebuilt 0

# The release profile strips debug information; there is nothing for debuginfo to collect.
%global debug_package %{nil}
%global appid io.github.zaun.Zaun
# The release version as tagged; RPM orders pre-releases with "~" (1.0.0-rc.1 -> 1.0.0~rc.1).
%global upstream_version 0.1.0
%{!?bash_completions_dir:%global bash_completions_dir %{_datadir}/bash-completion/completions}
%{!?zsh_completions_dir:%global zsh_completions_dir %{_datadir}/zsh/site-functions}
%{!?fish_completions_dir:%global fish_completions_dir %{_datadir}/fish/vendor_completions.d}
%{!?_metainfodir:%global _metainfodir %{_datadir}/metainfo}

Name:           zaun
Version:        %{lua: print((string.gsub(rpm.expand("%{upstream_version}"), "-", "~")))}
Release:        1%{?dist}
Summary:        Run Windows games in a per-game sandbox

License:        GPL-3.0-or-later AND AGPL-3.0-only AND LGPL-2.1-or-later AND LGPL-3.0-or-later AND OFL-1.1 AND MIT AND Apache-2.0 AND (MIT OR Apache-2.0) AND Unicode-3.0 AND BSD-2-Clause AND BSD-3-Clause AND ISC AND Zlib AND CDLA-Permissive-2.0
URL:            https://github.com/abdulrahman1s/zaun
Source0:        %{url}/archive/v%{upstream_version}/%{name}-%{upstream_version}.tar.gz
Source1:        %{url}/releases/download/v%{upstream_version}/%{name}-%{upstream_version}-vendor.tar.zst
Source2:        %{url}/releases/download/v%{upstream_version}/%{name}-%{upstream_version}-x86_64-linux.tar.zst

ExclusiveArch:  x86_64

# The Chat engine's prebuilt libraries (ADR-0033) are private to the Chat helper: they
# neither provide nor require anything for the rest of the system.
%global __provides_exclude_from ^%{_libexecdir}/%{name}/simplex/.*$
%global __requires_exclude_from ^%{_libexecdir}/%{name}/simplex/.*$

%if %{without prebuilt}
BuildRequires:  cargo >= 1.95
BuildRequires:  rust >= 1.95
BuildRequires:  gcc
BuildRequires:  glibc-static
BuildRequires:  git-core
%endif
BuildRequires:  binutils
BuildRequires:  zstd
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

Requires:       bubblewrap >= 0.8.0
# Not packaged by every Fedora release; the Doctor explains how to install it.
Recommends:     umu-launcher
Requires:       hicolor-icon-theme
# Loaded with dlopen at run time (RELEASE_STRATEGY §2.1), so not found by the dependency
# generator.
Requires:       libxkbcommon%{?_isa}
Requires:       (libwayland-client%{?_isa} or libX11%{?_isa})
Requires:       (vulkan-loader%{?_isa} or libglvnd-glx%{?_isa})
Requires:       systemd-libs%{?_isa}
Recommends:     libwayland-client%{?_isa}
Recommends:     libX11%{?_isa}
Recommends:     libXcursor%{?_isa}
Recommends:     libXi%{?_isa}
Recommends:     libXrandr%{?_isa}
Recommends:     libxkbcommon-x11%{?_isa}
Recommends:     xdg-desktop-portal
Recommends:     xdg-utils
Suggests:       mangohud
Suggests:       gamescope

# SQLite is compiled in (ADR-0017); the crates are vendored.
Provides:       bundled(sqlite) >= 3.37

%description
Zaun runs Windows games on Linux through umu-launcher and Proton. Open an
.exe from the file manager, answer one question about what the game may
access, and play. Every game runs inside its own bubblewrap sandbox with its
own Windows prefix, and only sees its own folder unless allowed more. Play
time statistics stay on the computer; there is no telemetry.

%prep
%if %{with prebuilt}
%setup -q -c -T
tar --zstd -xf %{SOURCE2} --strip-components=1
%else
%autosetup -n %{name}-%{upstream_version}
tar --zstd -xf %{SOURCE1}
mkdir -p .cargo
cat >>.cargo/config.toml <<'EOF'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
EOF
%endif

%build
%if %{without prebuilt}
# The tag archive has no .git; the release workflow records the commit in the vendor
# archive (vendor/COMMIT).
export ZAUN_COMMIT="$(cat vendor/COMMIT 2>/dev/null || echo unknown)"
export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-0}"
scripts/build-release.sh --build-only --offline --prefix %{_prefix} \
    --libexec-dir %{_libexecdir}/%{name} \
    --init-target x86_64-unknown-linux-gnu --jobs %{_smp_build_ncpus}
%endif

%install
%if %{with prebuilt}
# The release binary has /usr/libexec/zaun compiled in as the shim's location.
install -Dm755 libexec/zaun/zaun %{buildroot}%{_bindir}/zaun
for f in zaun-init zaun-overlay zaun-chat; do
    install -Dm755 libexec/zaun/$f %{buildroot}%{_libexecdir}/%{name}/$f
done
install -d %{buildroot}%{_libexecdir}/%{name}/simplex
install -m644 libexec/zaun/simplex/* %{buildroot}%{_libexecdir}/%{name}/simplex/
mkdir -p %{buildroot}%{_datadir}
cp -R share/. %{buildroot}%{_datadir}/
%else
scripts/build-release.sh --stage-only --offline --prefix %{_prefix} \
    --libexec-dir %{_libexecdir}/%{name} --init-target x86_64-unknown-linux-gnu \
    --destdir %{buildroot}
%endif
# Licences and docs are listed with %%license and %%doc below.
mkdir -p licenses docs
mv %{buildroot}%{_datadir}/licenses/%{name}/* licenses/
mv %{buildroot}%{_datadir}/doc/%{name}/* docs/
rm -rf %{buildroot}%{_datadir}/licenses/%{name} %{buildroot}%{_datadir}/doc/%{name}

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/%{appid}.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/%{appid}.metainfo.xml
%{buildroot}%{_bindir}/zaun --version

%files
%license licenses/*
%doc docs/*
%{_bindir}/zaun
%dir %{_libexecdir}/%{name}
%{_libexecdir}/%{name}/zaun-init
%{_libexecdir}/%{name}/zaun-overlay
%{_libexecdir}/%{name}/zaun-chat
%if %{with prebuilt}
%{_libexecdir}/%{name}/simplex/
%endif
%{_datadir}/applications/%{appid}.desktop
%{_metainfodir}/%{appid}.metainfo.xml
%{_datadir}/icons/hicolor/*/apps/%{appid}.png
%{_datadir}/icons/hicolor/scalable/apps/%{appid}.svg
%{_datadir}/icons/hicolor/symbolic/apps/%{appid}-symbolic.svg
%{_datadir}/systemd/user/zaun.service
%{_mandir}/man1/zaun*.1*
%{bash_completions_dir}/zaun
%{zsh_completions_dir}/_zaun
%{fish_completions_dir}/zaun.fish

%changelog
* Thu Nov 12 2026 Abdulrahman <mail@abdulrahman.dev> - 0.1.0-1
- First package.
