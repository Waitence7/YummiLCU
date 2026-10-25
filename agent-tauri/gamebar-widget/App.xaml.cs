using System;
using System.Collections.Generic;
using System.IO;
using Microsoft.Gaming.XboxGameBar;
using Windows.ApplicationModel.Activation;
using Windows.UI.Xaml;
using Windows.UI.Xaml.Controls;
using Windows.Storage;

namespace YummiGameBarWidget
{
    sealed partial class App : Application
    {
        private readonly Dictionary<string, XboxGameBarWidget> widgets = new Dictionary<string, XboxGameBarWidget>();

        public App()
        {
            InitializeComponent();
            UnhandledException += (_, e) => LogFailure("unhandled", e.Exception);
            Suspending += (_, __) => widgets.Clear();
        }

        protected override void OnActivated(IActivatedEventArgs args)
        {
            try
            {
                if (args.Kind != ActivationKind.Protocol || !(args is IProtocolActivatedEventArgs protocol) || protocol.Uri.Scheme != "ms-gamebarwidget")
                    return;

                var widgetArgs = args as XboxGameBarWidgetActivatedEventArgs;
                if (widgetArgs == null || !widgetArgs.IsLaunchActivation)
                    return;

                var frame = new Frame();
                Window.Current.Content = frame;
                var widget = new XboxGameBarWidget(widgetArgs, Window.Current.CoreWindow, frame);
                widgets[widgetArgs.AppExtensionId] = widget;

                switch (widgetArgs.AppExtensionId)
                {
                    case "YummiLCU":
                        frame.Navigate(typeof(Widget), widget);
                        break;
                    case "YummiLCUController":
                        frame.Navigate(typeof(Controller), widget);
                        break;
                    case "YummiLCUSettings":
                        frame.Navigate(typeof(Settings), widget);
                        break;
                    default:
                        widget.Close();
                        return;
                }

                var extensionId = widgetArgs.AppExtensionId;
                Window.Current.Closed += (_, __) => widgets.Remove(extensionId);
                Window.Current.Activate();
            }
            catch (Exception ex)
            {
                LogFailure("activation", ex);
                throw;
            }
        }

        private static void LogFailure(string stage, Exception error)
        {
            try
            {
                var path = Path.Combine(ApplicationData.Current.LocalFolder.Path, "widget-crash.log");
                var line = $"[{DateTimeOffset.Now:O}] {stage}: {error.GetType().FullName} 0x{error.HResult:X8} {error.Message}{Environment.NewLine}{error.StackTrace}{Environment.NewLine}";
                File.AppendAllText(path, line);
            }
            catch { }
        }
    }
}
