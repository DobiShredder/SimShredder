# Installing unsigned SimShredder releases

SimShredder `0.x` packages are distributed only through this repository's GitHub Releases and are not signed with Apple Developer ID or Windows Authenticode. Verify the release checksum before opening a package. Do not download a package from a mirror or disable operating-system security features globally.

## macOS 26 (Apple Silicon)

1. Download the `aarch64.dmg` file and `SHA256SUMS` from the same GitHub Release.
2. In Terminal, run `shasum -a 256 SimShredder-v0.x.y-aarch64.dmg` with the downloaded filename and verify that the value matches the DMG entry in `SHA256SUMS`.
3. Open the DMG and copy SimShredder to a directory writable by your user.
4. Try to open SimShredder once. If macOS blocks it because the developer cannot be verified, open **System Settings → Privacy & Security**, find the blocked SimShredder entry, and select **Open Anyway**. Confirm only after checking the checksum.

Apple documents that the exception button is available for about one hour after the blocked launch and asks for the current user's login password. The option may be unavailable on a Mac managed by an administrator or IT department. SimShredder does not remove quarantine attributes or change Gatekeeper settings for you.

## Windows x64

1. Download the `windows-x64-setup.exe` file and `SHA256SUMS` from the same GitHub Release.
2. In PowerShell, run `Get-FileHash -Algorithm SHA256 SimShredder-v0.x.y-windows-x64-setup.exe` with the downloaded filename and verify that the value matches the installer entry in `SHA256SUMS`.
3. Run the installer. It installs for the current user under `%LOCALAPPDATA%` and does not request machine-wide installation.
4. If Microsoft Defender SmartScreen shows a reputation warning and offers a **Run anyway** choice, continue only after checking the checksum.

Windows Smart App Control can block an unsigned application without offering an app-specific exception. SimShredder does not ask users to disable Smart App Control and cannot run where device policy prohibits unsigned applications.

---

# 서명되지 않은 SimShredder 설치

SimShredder `0.x` 설치본은 이 저장소의 GitHub Releases에서만 배포하며 Apple Developer ID 또는 Windows Authenticode로 서명하지 않습니다. 설치본을 열기 전에 같은 release의 checksum을 확인하세요. Mirror에서 설치본을 받거나 운영체제 보안 기능을 전체적으로 끄지 마세요.

## macOS 26 Apple Silicon

1. 같은 GitHub Release에서 `aarch64.dmg`와 `SHA256SUMS`를 받습니다.
2. 받은 파일 이름으로 `shasum -a 256 SimShredder-v0.x.y-aarch64.dmg`를 실행하고 결과가 `SHA256SUMS`의 DMG 항목과 일치하는지 확인합니다.
3. DMG를 열고 SimShredder를 현재 사용자가 쓸 수 있는 폴더로 복사합니다.
4. SimShredder를 한 번 실행합니다. 확인되지 않은 개발자라는 이유로 차단되면 **시스템 설정 → 개인정보 보호 및 보안**에서 차단된 SimShredder 항목의 **확인 없이 열기**를 선택합니다. Checksum을 확인한 경우에만 실행을 승인합니다.

Apple 안내에 따르면 이 버튼은 차단된 실행 후 약 한 시간 동안 표시되며 현재 사용자의 로그인 암호를 요구합니다. 관리자나 IT 부서가 관리하는 Mac에서는 이 선택지를 사용할 수 없을 수 있습니다. SimShredder는 quarantine 속성을 제거하거나 Gatekeeper 설정을 변경하지 않습니다.

## Windows x64

1. 같은 GitHub Release에서 `windows-x64-setup.exe`와 `SHA256SUMS`를 받습니다.
2. 받은 파일 이름으로 `Get-FileHash -Algorithm SHA256 SimShredder-v0.x.y-windows-x64-setup.exe`를 실행하고 결과가 `SHA256SUMS`의 설치본 항목과 일치하는지 확인합니다.
3. 설치본을 실행합니다. 현재 사용자용 `%LOCALAPPDATA%`에 설치하며 시스템 전체 설치 권한을 요청하지 않습니다.
4. Microsoft Defender SmartScreen이 평판 경고와 **실행** 선택지를 표시하는 경우 checksum을 확인한 뒤에만 계속합니다.

Windows Smart App Control은 앱별 예외 없이 unsigned application을 차단할 수 있습니다. SimShredder는 Smart App Control 비활성화를 요구하지 않으며 장치 정책이 unsigned application을 금지하는 환경에서는 실행할 수 없습니다.
