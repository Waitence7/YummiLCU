using System;
using Windows.Storage;
namespace YummiGameBarWidget
{
    internal static class YoutubePreferences
    {
        internal static string Video { get { object v; return ApplicationData.Current.LocalSettings.Values.TryGetValue("YoutubeWidgetVideo", out v) && v is string ? (string)v : ""; } set { Set("YoutubeWidgetVideo", value ?? ""); } }
        internal static double Volume { get { object v; return ApplicationData.Current.LocalSettings.Values.TryGetValue("YoutubeWidgetVolume", out v) && v is double ? Math.Max(0, Math.Min(100, (double)v)) : 60.0; } set { Set("YoutubeWidgetVolume", Math.Max(0, Math.Min(100, value))); } }
        internal static bool VideoOnlyMode { get { object v; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("YoutubeVideoOnlyMode", out v) || !(v is bool) || (bool)v; } set { Set("YoutubeVideoOnlyMode", value); } }
        internal static string Action { get { object v; return ApplicationData.Current.LocalSettings.Values.TryGetValue("YoutubeWidgetAction", out v) && v is string ? (string)v : ""; } }
        internal static long ActionRequest { get { object v; return ApplicationData.Current.LocalSettings.Values.TryGetValue("YoutubeWidgetActionRequest", out v) && v is long ? (long)v : 0; } }
        internal static void RequestAction(string action) { ApplicationData.Current.LocalSettings.Values["YoutubeWidgetAction"] = action ?? ""; ApplicationData.Current.LocalSettings.Values["YoutubeWidgetActionRequest"] = DateTimeOffset.UtcNow.Ticks; ApplicationData.Current.SignalDataChanged(); }
        private static void Set(string key, object value) { ApplicationData.Current.LocalSettings.Values[key] = value; ApplicationData.Current.SignalDataChanged(); }
    }
}
