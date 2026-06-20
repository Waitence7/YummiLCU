using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;

namespace YummiLcu.Core;

/// <summary>Discord OAuth 세션(session_id + ws_token)을 PC에 저장해 재실행 시 브라우저 로그인 생략.</summary>
public static class AgentSessionStore
{
    private static readonly JsonSerializerOptions JsonOpts = new() { WriteIndented = true };
    private const int CurrentStoreVersion = 3;

    public sealed record SavedSession(
        string SessionId,
        string WsToken,
        DateTimeOffset SavedAtUtc,
        string RelayBaseUrl);

    private sealed record EncryptedStoreFile(int V, string Payload);

    private static string StorePath =>
        Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
            "YummiAgent",
            "relay-session.json");

    public static SavedSession CreateNew(string relayBaseUrl) =>
        new(
            Guid.NewGuid().ToString(),
            Convert.ToBase64String(RandomNumberGenerator.GetBytes(32)),
            DateTimeOffset.UtcNow,
            NormalizeRelayBaseUrl(relayBaseUrl));

    public static SavedSession? Load(AgentConfig config)
    {
        try
        {
            if (!File.Exists(StorePath)) return null;
            var json = File.ReadAllText(StorePath);
            var encrypted = JsonSerializer.Deserialize<EncryptedStoreFile>(json);
            if (encrypted?.V == CurrentStoreVersion && !string.IsNullOrWhiteSpace(encrypted.Payload))
            {
                var plain = UnprotectPayload(encrypted.Payload);
                if (plain is null) return null;
                return DeserializeSession(plain, config);
            }
            Clear();
            return null;
        }
        catch
        {
            return null;
        }
    }

    public static void Save(string sessionId, string wsToken, string relayBaseUrl)
    {
        try
        {
            var dir = Path.GetDirectoryName(StorePath);
            if (!string.IsNullOrEmpty(dir))
                Directory.CreateDirectory(dir);
            var inner = JsonSerializer.Serialize(
                new SavedSession(
                    sessionId,
                    wsToken,
                    DateTimeOffset.UtcNow,
                    NormalizeRelayBaseUrl(relayBaseUrl)),
                JsonOpts);
            var payload = ProtectPayload(inner);
            if (payload is null)
                return;
            var outer = JsonSerializer.Serialize(new EncryptedStoreFile(CurrentStoreVersion, payload), JsonOpts);
            File.WriteAllText(StorePath, outer);
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

    private static SavedSession? DeserializeSession(string json, AgentConfig config)
    {
        var row = JsonSerializer.Deserialize<SavedSession>(json);
        if (row is null ||
            string.IsNullOrWhiteSpace(row.SessionId) ||
            string.IsNullOrWhiteSpace(row.WsToken) ||
            row.WsToken.Length < 16)
            return null;
        if (row.SavedAtUtc == default)
            return null;
        if (!string.Equals(
                row.RelayBaseUrl,
                NormalizeRelayBaseUrl(config.RelayPublicBaseUrl),
                StringComparison.OrdinalIgnoreCase))
            return null;
        var maxAgeDays = Math.Max(config.SavedSessionMaxAgeDays, 0);
        if (maxAgeDays == 0)
            return null;
        if (DateTimeOffset.UtcNow - row.SavedAtUtc > TimeSpan.FromDays(maxAgeDays))
            return null;
        return row;
    }

    private static string NormalizeRelayBaseUrl(string relayBaseUrl) =>
        relayBaseUrl.Trim().TrimEnd('/').ToLowerInvariant();

    private static string? ProtectPayload(string plain)
    {
        if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return null;
        try
        {
            var bytes = Encoding.UTF8.GetBytes(plain);
            var protectedBytes = ProtectedData.Protect(bytes, null, DataProtectionScope.CurrentUser);
            return Convert.ToBase64String(protectedBytes);
        }
        catch
        {
            return null;
        }
    }

    private static string? UnprotectPayload(string payloadB64)
    {
        if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return null;
        try
        {
            var protectedBytes = Convert.FromBase64String(payloadB64);
            var bytes = ProtectedData.Unprotect(protectedBytes, null, DataProtectionScope.CurrentUser);
            return Encoding.UTF8.GetString(bytes);
        }
        catch
        {
            return null;
        }
    }
}
