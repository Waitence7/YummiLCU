using System.Diagnostics;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;

namespace YummiLcu.Core;

public static class AgentUpdater
{
    private const string ExeName = "YummiLcu.App.exe";

    public static async Task<(bool Started, string Message)> DownloadAndApplyAsync(
        string zipUrl, string version, string? expectedSha256Hex = null, CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(zipUrl))
            return (false, "zip URL 없음");
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

        if (!string.IsNullOrWhiteSpace(expectedSha256Hex))
        {
            if (!VerifySha256File(zipPath, expectedSha256Hex))
                return (false, "업데이트 zip SHA-256 검증 실패 (manifest sha256 확인)");
        }

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

        return (true, $"업데이트 {version} 적용 중…");
    }

    private static bool VerifySha256File(string filePath, string expectedHex)
    {
        var expected = expectedHex.Trim().Replace("-", "").ToLowerInvariant();
        if (expected.Length != 64)
            return false;
        var hash = SHA256.HashData(File.ReadAllBytes(filePath));
        byte[] expectedBytes;
        try
        {
            expectedBytes = Convert.FromHexString(expected);
        }
        catch (FormatException)
        {
            return false;
        }
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
