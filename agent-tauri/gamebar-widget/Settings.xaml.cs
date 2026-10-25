using Microsoft.Gaming.XboxGameBar;
using Windows.UI.Xaml.Controls;
using Windows.UI.Xaml.Navigation;
namespace YummiGameBarWidget
{
    public sealed partial class Settings : Page
    {
        private XboxGameBarWidget widget;
        public Settings() { InitializeComponent(); }
        protected override void OnNavigatedTo(NavigationEventArgs e)
        {
            widget = e.Parameter as XboxGameBarWidget;
            base.OnNavigatedTo(e);
        }
    }
}
