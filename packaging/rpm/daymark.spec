%global debug_package %{nil}

Name:           daymark
Version:        0.1.1
Release:        1
Summary:        Quiet local screen time journal for GNOME

License:        MIT
URL:            https://github.com/101xAnshu/daymark
Source0:        %{name}-%{version}.tar.gz
BuildArch:      x86_64

Requires:       glibc >= 2.35
Requires:       libgcc
Recommends:     gnome-shell >= 48
Recommends:     gnome-shell-extension-appindicator

%description
Daymark records how long each application is used and presents a calm daily
view, the shape of the week, and fourteen days of history. Everything is
stored in a small SQLite file on the local machine.

A tiny GNOME Shell companion extension reports focus, idle and lock state.
Daymark opens no web view, makes no network requests, and needs no account.

GNOME Shell 48 or newer is required; on Fedora that means 42 or newer.

This package installs the application, desktop entry, icon and the companion
extension.

%prep
%setup -q

%build

%install
rm -rf %{buildroot}
install -d %{buildroot}
cp -a . %{buildroot}/

%files
%{_bindir}/daymark
%{_datadir}/applications/daymark.desktop
%{_datadir}/icons/hicolor/scalable/apps/daymark.svg
%{_datadir}/gnome-shell/extensions/daymark@daymark.app/
%{_docdir}/daymark/copyright
%{_docdir}/daymark/changelog
/usr/lib/systemd/user/daymark.service

%changelog
* Mon Oct 26 2026 Daymark contributors <maintainers@daymark.app> - 0.1.1-1
- Release 0.1.1.
