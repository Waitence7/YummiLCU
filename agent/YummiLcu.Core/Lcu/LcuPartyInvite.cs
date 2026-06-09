namespace YummiLcu.Core.Lcu;

public static class LcuPartyInvite
{
    public static bool TryParseRiotId(string riotId, out string gameName, out string tagLine)
    {
        gameName = "";
        tagLine = "";
        var text = (riotId ?? "").Trim();
        var idx = text.LastIndexOf('#');
        if (idx <= 0 || idx >= text.Length - 1)
            return false;
        gameName = text[..idx].Trim();
        tagLine = text[(idx + 1)..].Trim();
        return gameName.Length > 0 && tagLine.Length > 0;
    }

    public static string RiotKey(string gameName, string tagLine) =>
        $"{gameName.Trim()}#{tagLine.Trim()}".ToLowerInvariant();
}
