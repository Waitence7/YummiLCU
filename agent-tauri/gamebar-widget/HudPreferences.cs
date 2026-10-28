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
        internal static bool LcuPanelVisible
        {
            get { object value; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("LcuPanelVisible", out value) || !(value is bool) || (bool)value; }
            set { Set("LcuPanelVisible", value); }
        }
        internal static bool PartyPanelVisible
        {
            get { object value; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("PartyPanelVisible", out value) || !(value is bool) || (bool)value; }
            set { Set("PartyPanelVisible", value); }
        }
        internal static bool LcuPanelExpanded
        {
            get { object value; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("LcuPanelExpanded", out value) || !(value is bool) || (bool)value; }
            set { Set("LcuPanelExpanded", value); }
        }
        internal static bool PartyPanelExpanded
        {
            get { object value; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("PartyPanelExpanded", out value) || !(value is bool) || (bool)value; }
            set { Set("PartyPanelExpanded", value); }
        }
        internal static bool YoutubePanelExpanded
        {
            get { object value; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("YoutubePanelExpanded", out value) || !(value is bool) || (bool)value; }
            set { Set("YoutubePanelExpanded", value); }
        }
        internal static bool NativeInputPanelVisible
        {
            get { object value; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("NativeInputPanelVisible", out value) || !(value is bool) || (bool)value; }
            set { Set("NativeInputPanelVisible", value); }
        }
        internal static bool NativeInputPanelExpanded
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("NativeInputPanelExpanded", out value) && value is bool && (bool)value; }
            set { Set("NativeInputPanelExpanded", value); }
        }
        internal static bool NativeCursorMarkersVisible
        {
            get { object value; return !ApplicationData.Current.LocalSettings.Values.TryGetValue("NativeCursorMarkersVisible", out value) || !(value is bool) || (bool)value; }
            set { Set("NativeCursorMarkersVisible", value); }
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
        internal static bool InitialFitDone
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("HudInitialFitDone", out value) && value is bool && (bool)value; }
        }
        internal static void MarkInitialFitDone() { Set("HudInitialFitDone", true); }
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
