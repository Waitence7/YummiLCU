using System;
using Microsoft.Gaming.XboxGameBar;
using Windows.ApplicationModel.Activation;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;

namespace YummiGameBarWidget
{
    sealed partial class App : Application
    {
        private XboxGameBarWidget gameBarWidget;

        public App()
        {
            InitializeComponent();
            Suspending += (_, __) => gameBarWidget = null;
        }

        protected override void OnActivated(IActivatedEventArgs args)
        {
            if (args.Kind != ActivationKind.Protocol || !(args is IProtocolActivatedEventArgs protocol) || protocol.Uri.Scheme != "ms-gamebarwidget")
                return;

            var widgetArgs = args as XboxGameBarWidgetActivatedEventArgs;
            if (widgetArgs == null || !widgetArgs.IsLaunchActivation)
                return;

            var frame = new Frame();
            Window.Current.Content = frame;
            gameBarWidget = new XboxGameBarWidget(widgetArgs, Window.Current.CoreWindow, frame);
            frame.Navigate(typeof(Widget));
            Window.Current.Closed += (_, __) => gameBarWidget = null;
            Window.Current.Activate();
        }
    }
}
