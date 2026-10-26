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
using Windows.UI.Xaml.Input;
using Windows.System;

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
                LcuVisibleToggle.IsOn = HudPreferences.LcuPanelVisible;
                PartyVisibleToggle.IsOn = HudPreferences.PartyPanelVisible;
                YoutubeVisibleToggle.IsOn = YoutubePreferences.Visible;
                YoutubeUrlBox.Text = YoutubePreferences.Video;
                YoutubeVolumeSlider.Value = YoutubePreferences.Volume;
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
        private void LcuVisibleToggle_Toggled(object sender, RoutedEventArgs args)
        {
            if (preferencesLoaded) HudPreferences.LcuPanelVisible = LcuVisibleToggle.IsOn;
        }
        private void PartyVisibleToggle_Toggled(object sender, RoutedEventArgs args)
        {
            if (preferencesLoaded) HudPreferences.PartyPanelVisible = PartyVisibleToggle.IsOn;
        }
        private void CompactHudButton_Click(object sender, RoutedEventArgs args)
        {
            HudPreferences.RequestCompact();
            PreferenceStatusText.Text = "HUD를 가운데의 작은 크기로 줄입니다. 제목줄에서 핀과 위치를 조절해 주세요.";
        }
        private void FitHudButton_Click(object sender, RoutedEventArgs args)
        {
            HudPreferences.RequestFit();
            PreferenceStatusText.Text = "Game Bar가 허용하는 최대 크기로 맞춥니다.";
        }

        private void YoutubeVisibleToggle_Toggled(object sender, RoutedEventArgs args)
        {
            if (preferencesLoaded) YoutubePreferences.Visible = YoutubeVisibleToggle.IsOn;
        }
        private void YoutubeApplyButton_Click(object sender, RoutedEventArgs args) { ApplyYoutubeVideo(); }
        private void YoutubeUrlBox_KeyDown(object sender, KeyRoutedEventArgs args)
        {
            if (args.Key != VirtualKey.Enter) return;
            args.Handled = true;
            ApplyYoutubeVideo();
        }
        private void ApplyYoutubeVideo()
        {
            var value = (YoutubeUrlBox.Text ?? "").Trim();
            if (value.Length == 0) { YoutubeSettingsStatusText.Text = "YouTube 링크 또는 영상 ID를 입력해 주세요."; return; }
            YoutubePreferences.Video = value;
            YoutubePreferences.Visible = true;
            HudPreferences.YoutubePanelExpanded = true;
            YoutubeVisibleToggle.IsOn = true;
            YoutubeSettingsStatusText.Text = "영상을 적용하고 YouTube 패널을 표시했습니다.";
        }
        private void YoutubePlayButton_Click(object sender, RoutedEventArgs args)
        {
            YoutubePreferences.RequestAction("play");
            YoutubeSettingsStatusText.Text = "재생 명령을 보냈습니다.";
        }
        private void YoutubePauseButton_Click(object sender, RoutedEventArgs args)
        {
            YoutubePreferences.RequestAction("pause");
            YoutubeSettingsStatusText.Text = "일시정지 명령을 보냈습니다.";
        }
        private void YoutubeVolumeSlider_ValueChanged(object sender, RangeBaseValueChangedEventArgs args)
        {
            if (preferencesLoaded) YoutubePreferences.Volume = args.NewValue;
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
