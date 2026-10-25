using System;
using Microsoft.Gaming.XboxGameBar;
using Windows.UI.Core;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Navigation;

namespace YummiGameBarWidget
{
    public sealed partial class Controller : Page
    {
        private XboxGameBarWidget widget;
        public Controller() { InitializeComponent(); }
        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            widget = e.Parameter as XboxGameBarWidget;
            if (widget != null)
            {
                widget.PinningSupported = false;
                widget.SettingsClicked += WidgetSettingsClicked;
            }
            base.OnNavigatedTo(e);
        }
        protected override void OnNavigatedFrom(NavigationEventArgs e)
        {
            if (widget != null) widget.SettingsClicked -= WidgetSettingsClicked;
            base.OnNavigatedFrom(e);
        }
        private async void WidgetSettingsClicked(XboxGameBarWidget sender, object args)
        {
            try
            {
                await Dispatcher.RunAsync(CoreDispatcherPriority.Normal, async () =>
                {
                    try { await widget.ActivateSettingsAsync(); }
                    catch { OpenSettingsButton.Content = "Game Bar에서 다시 열어 주세요"; }
                });
            }
            catch { }
        }
        private async void OpenSettingsButton_Click(object sender, RoutedEventArgs e)
        {
            if (widget == null) return;
            try { await widget.ActivateSettingsAsync(); }
            catch { OpenSettingsButton.Content = "Win+G에서 설정을 다시 열어 주세요"; }
        }
    }
}
