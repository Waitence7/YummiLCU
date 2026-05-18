using System.Diagnostics;
using System.IO.Compression;
using System.Text;

namespace YummiLcu.Agent;

/// <summary>zip 다운로드 → 보조 cmd로 교체 → 에이전트 재시작.</summary>
internal static class AgentUpdater
{
    private const string ExeName = "YummiLcu.Agent.exe";

    public static async Task<(bool Started, string Message)> DownloadAndApplyAsync(
        string zipUrl,
        string version,
        CancellationToken ct = default)
    {
        if (string.IsNullOrWhiteSpace(zipUrl))
            return (false, "zip URL 없음");

        var targetDir = Path.GetFullPath(AppContext.BaseDirectory).TrimEnd(Path.DirectorySeparatorChar);
        var workDir = Path.Combine(Path.GetTempPath(), "yummi-agent-update", version);
        Directory.CreateDirectory(workDir);

        var zipPath = Path.Combine(workDir, "update.zip");
        var extractDir = Path.Combine(workDir, "extract");

        try
        {
            using var http = new HttpClient { Timeout = TimeSpan.FromMinutes(8) };
            await using var stream = await http.GetStreamAsync(zipUrl, ct);
            await using var file = File.Create(zipPath);
            await stream.CopyToAsync(file, ct);
        }
        catch (Exception ex)
        {
            return (false, $"다운로드 실패: {ex.Message}");
        }

        try
        {
            if (Directory.Exists(extractDir))
                Directory.Delete(extractDir, true);
            Directory.CreateDirectory(extractDir);
            ZipFile.ExtractToDirectory(zipPath, extractDir);
        }
        catch (Exception ex)
        {
            return (false, $"압축 해제 실패: {ex.Message}");
        }

        var sourceDir = ResolvePublishRoot(extractDir);
        if (sourceDir is null)
            return (false, "zip 안에 YummiLcu.Agent.exe 없음");

        var scriptPath = Path.Combine(workDir, "apply-update.cmd");
        var script = BuildUpdateScript(sourceDir, targetDir);
        await File.WriteAllTextAsync(scriptPath, script, Encoding.Default, ct);

        Process.Start(new ProcessStartInfo
        {
            FileName = "cmd.exe",
            Arguments = $"/c \"\"{scriptPath}\"\"",
            UseShellExecute = false,
            CreateNoWindow = true,
        });

        return (true, $"업데이트 {version} 적용 중… 잠시 후 재시작됩니다.");
    }

    private static string? ResolvePublishRoot(string extractDir)
    {
        var direct = Path.Combine(extractDir, ExeName);
        if (File.Exists(direct))
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
            if exist "{dst}\agent.json" (
              robocopy "{src}" "{dst}" /E /XC /XN /XO /XF agent.json >nul
            ) else (
              robocopy "{src}" "{dst}" /E /XC /XN /XO >nul
            )
            if errorlevel 8 exit /b 1
            if not exist "{dst}\agent.json" if exist "{src}\agent.json" copy /Y "{src}\agent.json" "{dst}\agent.json" >nul
            start "" "{exe}"
            del "%~f0"
            """;
    }
}
