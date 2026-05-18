using System.Globalization;
using System.Text;

namespace YummiLcu.Agent;

internal static class StatusMessageHelper
{
    /// <summary>기본 상메 — Unicode 볼드(산세리프).</summary>
    public const string DefaultYummiClient = "𝗬𝘂𝗺𝗺𝗶 𝗖𝗹𝗶𝗲𝗻𝘁";

    public static string Normalize(string? text)
    {
        if (string.IsNullOrWhiteSpace(text))
            return DefaultYummiClient;
        return text.Replace("\r\n", "\n", StringComparison.Ordinal).Replace('\r', '\n');
    }

    /// <summary>제어 문자 제외, 줄바꿈 허용. 클라 상메 길이 제한.</summary>
    public static bool TryValidate(string text, out string error)
    {
        if (string.IsNullOrEmpty(text))
        {
            error = "상메가 비어 있습니다.";
            return false;
        }
        if (text.Length > 128)
        {
            error = "상메는 128자 이하여야 합니다.";
            return false;
        }
        var sb = new StringBuilder();
        foreach (var c in text)
        {
            if (c is '\n')
                continue;
            if (char.IsControl(c))
            {
                error = "제어 문자는 사용할 수 없습니다.";
                return false;
            }
            var cat = CharUnicodeInfo.GetUnicodeCategory(c);
            if (cat is UnicodeCategory.Surrogate or UnicodeCategory.PrivateUse)
            {
                error = "유니코드 서로게이트/PUA 문자는 사용할 수 없습니다.";
                return false;
            }
        }
        error = "";
        return true;
    }
}
