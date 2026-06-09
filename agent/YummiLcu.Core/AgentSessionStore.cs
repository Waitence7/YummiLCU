using System.Security.Cryptography;
using System.Text.Json;

namespace YummiLcu.Core;

/// <summary>Discord OAuth 세션(session_id + ws_token)을 PC에 저장해 재실행 시 브라우저 로그인 생략.</summary>
public static class AgentSessionStore
{
    private static readonly JsonSerializerOptions JsonOpts = new() { WriteIndented = true };

    public sealed record SavedSession(string SessionId, string WsToken);

    private static string StorePath =>
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "YummiAgent",
            "relay-session.json");

    public static SavedSession CreateNew() =>
        new(Guid.NewGuid().ToString(), Convert.ToBase64String(RandomNumberGenerator.GetBytes(32)));

    public static SavedSession? Load()
    {
        try
        {
            if (!File.Exists(StorePath)) return null;
            var json = File.ReadAllText(StorePath);
            var row = JsonSerializer.Deserialize<SavedSession>(json);
            if (row is null ||
                string.IsNullOrWhiteSpace(row.SessionId) ||
                string.IsNullOrWhiteSpace(row.WsToken) ||
                row.WsToken.Length < 16)
                return null;
            return row;
        }
        catch
        {
            return null;
        }
    }

    public static void Save(string sessionId, string wsToken)
    {
        try
        {
            var dir = Path.GetDirectoryName(StorePath);
            if (!string.IsNullOrEmpty(dir))
                Directory.CreateDirectory(dir);
            var json = JsonSerializer.Serialize(new SavedSession(sessionId, wsToken), JsonOpts);
            File.WriteAllText(StorePath, json);
        }
        catch
        {
            // ignore — 저장 실패해도 연결은 계속
        }
    }

    public static void Clear()
    {
        try
        {
            if (File.Exists(StorePath))
                File.Delete(StorePath);
        }
        catch
        {
            // ignore
        }
    }
}
