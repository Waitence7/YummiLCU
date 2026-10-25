using Microsoft.Gaming.XboxGameBar;
using Windows.System;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Controls.Primitives;
using Windows.UI.Xaml.Input;
using Windows.UI.Xaml.Navigation;
namespace YummiGameBarWidget
{
    public sealed partial class YoutubeSettings : Page
    {
        private bool loaded;
        private XboxGameBarWidget widget;
        public YoutubeSettings()
        {
            InitializeComponent();
            Loaded += (_, __) => { YoutubeUrlBox.Text = YoutubePreferences.Video; YoutubeVolumeSlider.Value = YoutubePreferences.Volume; loaded = true; };
        }
        protected override void OnNavigatedTo(NavigationEventArgs e) { widget = e.Parameter as XboxGameBarWidget; base.OnNavigatedTo(e); }
        private void YoutubeApplyButton_Click(object sender, RoutedEventArgs e) => ApplyVideo();
        private void YoutubeUrlBox_KeyDown(object sender, KeyRoutedEventArgs e) { if (e.Key != VirtualKey.Enter) return; e.Handled = true; ApplyVideo(); }
        private void ApplyVideo() { var value = (YoutubeUrlBox.Text ?? "").Trim(); if (string.IsNullOrWhiteSpace(value)) { StatusText.Text = "YouTube 링크 또는 영상 ID를 입력해 주세요."; return; } YoutubePreferences.Video = value; StatusText.Text = "영상 설정을 적용했습니다."; }
        private void YoutubePlayButton_Click(object sender, RoutedEventArgs e) { YoutubePreferences.RequestAction("play"); StatusText.Text = "재생"; }
        private void YoutubePauseButton_Click(object sender, RoutedEventArgs e) { YoutubePreferences.RequestAction("pause"); StatusText.Text = "일시정지"; }
        private void YoutubeVolumeSlider_ValueChanged(object sender, RangeBaseValueChangedEventArgs e) { if (loaded) YoutubePreferences.Volume = e.NewValue; }
    }
}
