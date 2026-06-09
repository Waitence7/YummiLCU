namespace YummiLcu.Core;

/// <summary>%LocalAppData%\YummiAgent\agent.log — UI 로그와 병행.</summary>
public static class AgentFileLogger
{
    private const long MaxBytes = 2 * 1024 * 1024;
    private static readonly object Gate = new();

    private static string LogDirectory =>
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "YummiAgent");

    private static string LogPath => Path.Combine(LogDirectory, "agent.log");

    public static void Write(string line)
    {
        if (string.IsNullOrWhiteSpace(line)) return;
        var entry = $"[{DateTime.Now:yyyy-MM-dd HH:mm:ss}] {line}";
        try
        {
            lock (Gate)
            {
                Directory.CreateDirectory(LogDirectory);
                RotateIfNeeded();
                File.AppendAllText(LogPath, entry + Environment.NewLine);
            }
        }
        catch
        {
            // ignore
        }
    }

    private static void RotateIfNeeded()
    {
        if (!File.Exists(LogPath)) return;
        var info = new FileInfo(LogPath);
        if (info.Length <= MaxBytes) return;
        var backup = LogPath + ".old";
        if (File.Exists(backup)) File.Delete(backup);
        File.Move(LogPath, backup);
    }
}
