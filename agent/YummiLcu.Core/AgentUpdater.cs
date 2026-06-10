using System.Diagnostics;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;

namespace YummiLcu.Core;

public static class AgentUpdater
{
    private const string ExeName = "YummiLcu.App.exe";
    private const string CoreDllName = "YummiLcu.Core.dll";

    /// <summary>슬림(프레임워크 의존) 배포 — Core.dll 이 함께 있음.</summary>
    public static bool IsFrameworkDependentLayout(string? baseDir = null)
    {
        var dir = baseDir ?? AppContext.BaseDirectory;
        return File.Exists(Path.Combine(dir, CoreDllName));
    }

    public static async Task<(bool Started, string Message)> DownloadAndApplyAsync(
        UpdateChecker.UpdateInfo info, CancellationToken ct = default)
    {
        var targetDir = Path.GetFullPath(AppContext.BaseDirectory).TrimEnd(Path.DirectorySeparatorChar);

        if (!IsFrameworkDependentLayout(targetDir))
        {
            var installer = info.PreferredDownloadUrl;
            if (!string.IsNullOrWhiteSpace(installer))
                return (false, "구버전(단일 exe) 설치입니다. 설치 프로그램으로 v" + info.Version + " 을 받아 주세요.");
            return (false, "자동 zip 업데이트는 슬림 설치(0.5.3+)에서만 지원됩니다.");
        }

        var usePatch = !string.IsNullOrWhiteSpace(info.PatchUrl)
            && !string.IsNullOrWhiteSpace(info.PatchFrom)
            && Version.TryParse(UpdateChecker.CurrentVersion, out var cur)
            && Version.TryParse(info.PatchFrom, out var from)
            && cur == from;

        var url = usePatch ? info.PatchUrl!.Trim() : (info.Url ?? "").Trim();
        var sha = usePatch ? info.PatchSha256 : info.Sha256;
        var label = usePatch ? $"패치 {info.PatchFrom}→{info.Version}" : $"전체 v{info.Version}";

        if (string.IsNullOrWhiteSpace(url))
            return (false, "업데이트 URL 없음");

        return await DownloadZipAndApplyAsync(url, info.Version, sha, label, ct);
    }

    public static async Task<(bool Started, string Message)> DownloadZipAndApplyAsync(
        string zipUrl, string version, string? expectedSha256Hex = null, string? label = null,
        CancellationToken ct = default)
    {
        if (!Uri.TryCreate(zipUrl.Trim(), UriKind.Absolute, out var uri) || uri.Scheme != Uri.UriSchemeHttps)
            return (false, "zip URL은 https 만 허용됩니다");

        var targetDir = Path.GetFullPath(AppContext.BaseDirectory).TrimEnd(Path.DirectorySeparatorChar);
        var workDir = Path.Combine(Path.GetTempPath(), "yummi-agent-update", version);
        Directory.CreateDirectory(workDir);

        var zipPath = Path.Combine(workDir, "update.zip");
        var extractDir = Path.Combine(workDir, "extract");

        try
        {
            using var http = new HttpClient { Timeout = TimeSpan.FromMinutes(8) };
            await using var stream = await http.GetStreamAsync(uri, ct);
            await using var file = File.Create(zipPath);
            await stream.CopyToAsync(file, ct);
        }
        catch (Exception ex)
        {
            return (false, $"다운로드 실패: {ex.Message}");
        }

        if (string.IsNullOrWhiteSpace(expectedSha256Hex))
            return (false, "manifest에 sha256 이 없어 업데이트를 거부했습니다");

        if (!VerifySha256File(zipPath, expectedSha256Hex))
            return (false, "업데이트 zip SHA-256 검증 실패");

        try
        {
            if (Directory.Exists(extractDir)) Directory.Delete(extractDir, true);
            Directory.CreateDirectory(extractDir);
            ZipFile.ExtractToDirectory(zipPath, extractDir);
        }
        catch (Exception ex)
        {
            return (false, $"압축 해제 실패: {ex.Message}");
        }

        var sourceDir = ResolvePublishRoot(extractDir);
        if (sourceDir is null)
            return (false, "zip 안에 YummiLcu.App.exe 없음");

        var scriptPath = Path.Combine(workDir, "apply-update.cmd");
        await File.WriteAllTextAsync(scriptPath, BuildUpdateScript(sourceDir, targetDir), Encoding.Default, ct);

        Process.Start(new ProcessStartInfo
        {
            FileName = "cmd.exe",
            Arguments = $"/c \"\"{scriptPath}\"\"",
            UseShellExecute = false,
            CreateNoWindow = true,
        });

        var msg = string.IsNullOrWhiteSpace(label) ? $"업데이트 {version} 적용 중…" : $"{label} 적용 중…";
        return (true, msg);
    }

    private static bool VerifySha256File(string filePath, string expectedHex)
    {
        var expected = expectedHex.Trim().Replace("-", "").ToLowerInvariant();
        if (expected.Length != 64) return false;
        var hash = SHA256.HashData(File.ReadAllBytes(filePath));
        byte[] expectedBytes;
        try { expectedBytes = Convert.FromHexString(expected); }
        catch (FormatException) { return false; }
        return CryptographicOperations.FixedTimeEquals(hash, expectedBytes);
    }

    private static string? ResolvePublishRoot(string extractDir)
    {
        if (File.Exists(Path.Combine(extractDir, ExeName)))
            return extractDir;
        foreach (var sub in Directory.EnumerateDirectories(extractDir))
        {
            if (File.Exists(Path.Combine(sub, ExeName)))
                return sub;
        }
        foreach (var exe in Directory.EnumerateFiles(extractDir, ExeName, SearchOption.AllDirectories))
            return Path.GetDirectoryName(exe);
        return null;
    }

    private static string BuildUpdateScript(string sourceDir, string targetDir)
    {
        var src = sourceDir.Replace("\"", "\"\"");
        var dst = targetDir.Replace("\"", "\"\"");
        var exe = Path.Combine(targetDir, ExeName).Replace("\"", "\"\"");
        return $"""
            @echo off
            timeout /t 2 /nobreak >nul
            robocopy "{src}" "{dst}" /E /XC /XN /XO /XF agent.json >nul
            if errorlevel 8 exit /b 1
            if not exist "{dst}\agent.json" if exist "{src}\agent.json" copy /Y "{src}\agent.json" "{dst}\agent.json" >nul
            start "" "{exe}"
            del "%~f0"
            """;
    }
}
