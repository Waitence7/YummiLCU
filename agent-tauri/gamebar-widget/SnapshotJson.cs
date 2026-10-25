using Windows.Data.Json;

namespace YummiGameBarWidget
{
    internal static class SnapshotJson
    {
        internal static string String(JsonObject json, string name, string fallback = "")
        {
            var value = json.GetNamedValue(name, null);
            return value != null && value.ValueType == JsonValueType.String ? value.GetString() : fallback;
        }
    }
}
