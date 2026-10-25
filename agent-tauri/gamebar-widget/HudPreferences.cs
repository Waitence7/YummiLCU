using System;
using Windows.Storage;

namespace YummiGameBarWidget
{
    internal static class HudPreferences
    {
        internal static bool Visible
        {
            get { object value; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("HudVisible", out value) || !(value is bool) || (bool)value; }
            set { Set("HudVisible", value); }
        }
        internal static bool CoverageTint
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("HudCoverageTint", out value) && value is bool && (bool)value; }
            set { Set("HudCoverageTint", value); }
        }
        internal static double Opacity
        {
            get
            {
                object value;
                if (!ApplicationData.Current.LocalSettings.Values.TryGetValue("HudOpacity", out value) || !(value is double)) return 1.0;
                return Math.Max(0.1, Math.Min(1.0, (double)value));
            }
            set { Set("HudOpacity", Math.Max(0.1, Math.Min(1.0, value))); }
        }
        internal static bool CompactLayout
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("HudCompactLayout", out value) && value is bool && (bool)value; }
        }
        internal static long FitRequest
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("HudFitRequest", out value) && value is long ? (long)value : 0; }
        }
        internal static void RequestFit()
        {
            Set("HudCompactLayout", false);
            Set("HudFitRequest", DateTimeOffset.UtcNow.Ticks);
        }
        internal static void RequestCompact()
        {
            Set("HudCompactLayout", true);
            Set("HudFitRequest", DateTimeOffset.UtcNow.Ticks);
        }

        private static void Set(string key, object value)
        {
            ApplicationData.Current.LocalSettings.Values[key] = value;
            ApplicationData.Current.SignalDataChanged();
        }
    }
}
