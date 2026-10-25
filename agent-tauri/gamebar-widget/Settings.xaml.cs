using System;
using System.IO;
using System.Threading;
using System.Text;
using System.Threading.Tasks;
using Microsoft.Gaming.XboxGameBar;
using Windows.Data.Json;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Controls.Primitives;
using Windows.UI.Xaml.Navigation;

namespace YummiGameBarWidget
{
    public sealed partial class Settings : Page
    {
        private const string CommandPipeName = @"LOCAL\YummiLcuGameBarCommandV1";
        private XboxGameBarWidget widget;

        private bool preferencesLoaded;
        public Settings()
        {
            InitializeComponent();
            Loaded += (_, __) =>
            {
                HudVisibleToggle.IsOn = HudPreferences.Visible;
                CoverageTintToggle.IsOn = HudPreferences.CoverageTint;
                HudOpacitySlider.Value = HudPreferences.Opacity * 100;
                preferencesLoaded = true;
            };
        }
        private void HudVisibleToggle_Toggled(object sender, RoutedEventArgs args)
        {
            if (preferencesLoaded) HudPreferences.Visible = HudVisibleToggle.IsOn;
        }
        private void CoverageTintToggle_Toggled(object sender, RoutedEventArgs args)
        {
            if (preferencesLoaded) HudPreferences.CoverageTint = CoverageTintToggle.IsOn;
        }
        private void HudOpacitySlider_ValueChanged(object sender, RangeBaseValueChangedEventArgs args)
        {
            if (preferencesLoaded) HudPreferences.Opacity = args.NewValue / 100;
        }
        private void FitHudButton_Click(object sender, RoutedEventArgs args)
        {
            HudPreferences.RequestFit();
            PreferenceStatusText.Text = "Game Bar가 허용하는 최대 크기로 맞춥니다.";
        }


        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            widget = e.Parameter as XboxGameBarWidget;
            base.OnNavigatedTo(e);
        }

        private async void CheckUpdateButton_Click(object sender, RoutedEventArgs e)
        {
            CheckUpdateButton.IsEnabled = false;
            UpdateStatusText.Text = "업데이트 확인 요청 중…";
            try
            {
                var response = await SendCommandAsync("{\"command\":\"check_overlay_update\"}");
                var json = JsonObject.Parse(response);
                var ok = json.GetNamedBoolean("ok", false);
                UpdateStatusText.Text = json.GetNamedString("message", ok ? "업데이트 확인을 시작했습니다." : "업데이트 확인에 실패했습니다.");
            }
            catch
            {
                UpdateStatusText.Text = "Yummi LCU Agent에 연결하지 못했습니다. Agent를 실행해 주세요.";
            }
            finally
            {
                CheckUpdateButton.IsEnabled = true;
            }
        }

        private static async Task<string> SendCommandAsync(string payload)
        {
            using (var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2)))
            using (var pipe = NativePipeClient.Open(CommandPipeName, FileAccess.ReadWrite))
            {
                using (var writer = new StreamWriter(pipe, new UTF8Encoding(false), 1024, true) { AutoFlush = true })
                {
                    await writer.WriteLineAsync(payload);
                    var line = await NativePipeClient.ReadLineAsync(pipe, timeout.Token);
                    if (string.IsNullOrWhiteSpace(line)) throw new IOException("empty command response");
                    return line;
                }
            }
        }
    }
}
