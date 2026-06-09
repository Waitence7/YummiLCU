using System.Text.Json;

namespace YummiLcu.Core.Lcu;

public readonly record struct ActionResult(bool Ok, string Message, JsonElement? Data = null)
{
    public static ActionResult FromBool(bool ok, string? okMsg = "ok", string? failMsg = "LCU 요청 실패") =>
        new(ok, ok ? (okMsg ?? "ok") : (failMsg ?? "LCU 요청 실패"));
}
