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
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("LcuPanelVisible", out value) && value is bool && (bool)value; }
            set { Set("LcuPanelVisible", value); }
        }
        internal static bool PartyPanelVisible
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("PartyPanelVisible", out value) && value is bool && (bool)value; }
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
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("NativeInputPanelVisible", out value) && value is bool && (bool)value; }
            set { Set("NativeInputPanelVisible", value); }
        }
        internal static bool NativeInputPanelExpanded
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("NativeInputPanelExpanded", out value) && value is bool && (bool)value; }
            set { Set("NativeInputPanelExpanded", value); }
        }
        internal static bool NativeCursorMarkersVisible
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("NativeCursorMarkersVisible", out value) && value is bool && (bool)value; }
            set { Set("NativeCursorMarkersVisible", value); }
        }
        internal static double VoiceChatMessageLifetimeSeconds
        {
            get
            {
                object value;
                if (!ApplicationData.Current.LocalSettings.Values.TryGetValue("VoiceChatMessageLifetimeSeconds", out value) || !(value is double)) return 10.0;
                return Math.Max(3.0, Math.Min(60.0, (double)value));
            }
            set { Set("VoiceChatMessageLifetimeSeconds", Math.Max(3.0, Math.Min(60.0, value))); }
        }
        internal static string VoiceChatBackgroundColor
        {
            get
            {
                object value;
                if (!ApplicationData.Current.LocalSettings.Values.TryGetValue("VoiceChatBackgroundColor", out value) || !(value is string)) return "#141020";
                var color = ((string)value).Trim();
                return color.Length == 7 && color[0] == '#' ? color : "#141020";
            }
            set { Set("VoiceChatBackgroundColor", string.IsNullOrWhiteSpace(value) ? "#141020" : value.Trim()); }
        }
        internal static double VoiceChatBackgroundOpacity
        {
            get
            {
                object value;
                if (!ApplicationData.Current.LocalSettings.Values.TryGetValue("VoiceChatBackgroundOpacity", out value) || !(value is double)) return 0.0;
                return Math.Max(0.0, Math.Min(1.0, (double)value));
            }
            set { Set("VoiceChatBackgroundOpacity", Math.Max(0.0, Math.Min(1.0, value))); }
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
        internal static bool PinTutorialCompleted
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("HudPinTutorialCompleted", out value) && value is bool && (bool)value; }
        }
        internal static void MarkPinTutorialCompleted() { Set("HudPinTutorialCompleted", true); }
        internal static bool ControlsTutorialCompleted
        {
            get { object value; return ApplicationData.Current.LocalSettings.Values.TryGetValue("HudControlsTutorialCompleted", out value) && value is bool && (bool)value; }
        }
        internal static void MarkControlsTutorialCompleted() { Set("HudControlsTutorialCompleted", true); }
        internal static void ResetTutorials()
        {
            ApplicationData.Current.LocalSettings.Values.Remove("HudPinTutorialCompleted");
            ApplicationData.Current.LocalSettings.Values.Remove("HudControlsTutorialCompleted");
            ApplicationData.Current.SignalDataChanged();
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

        internal static int ResetAllSavedStateForDevelopment()
        {
            var values = ApplicationData.Current.LocalSettings.Values;
            var count = values.Count;
            values.Clear();
            return count;
        }

        private static void Set(string key, object value)
        {
            ApplicationData.Current.LocalSettings.Values[key] = value;
            ApplicationData.Current.SignalDataChanged();
        }
    }
}
