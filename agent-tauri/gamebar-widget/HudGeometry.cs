using System;
using System.Threading.Tasks;

namespace YummiGameBarWidget
{
    internal static class HudGeometry
    {
        internal struct FitResult
        {
            internal double Width;
            internal double Height;
            internal bool Accepted;
        }

        // Probe only in response to a fit request. The host can reject sizes
        // below our manifest maximum because it reserves room for its chrome.
        internal static async Task<FitResult> FitLargestAsync(double width, double height,
            double minWidth, double minHeight, Func<double, double, Task<bool>> resize)
        {
            if (await resize(width, height))
                return new FitResult { Width = width, Height = height, Accepted = true };
            if (!await resize(minWidth, minHeight)) return new FitResult();
            double low = await resize(width, minHeight) ? width : minWidth, high = width;
            for (int attempt = 0; attempt < 14 && high - low > 1; attempt++)
            {
                double mid = Math.Floor((low + high) / 2);
                if (await resize(mid, minHeight)) low = mid;
                else high = mid;
            }
            double fittedWidth = low;
            low = await resize(fittedWidth, height) ? height : minHeight;
            high = height;
            for (int attempt = 0; attempt < 14 && high - low > 1; attempt++)
            {
                double mid = Math.Floor((low + high) / 2);
                if (await resize(fittedWidth, mid)) low = mid;
                else high = mid;
            }
            return new FitResult { Width = fittedWidth, Height = low,
                Accepted = await resize(fittedWidth, low) };
        }

        // Game Bar bounds and XAML content can use different coordinate scales.
        // Derive the conversion from the same live view instead of assuming DIPs.
        internal static double TargetInHostUnits(double displayDips, double hostSize, double contentDips, double minimum, double maximum)
        {
            double hostUnitsPerDip = hostSize > 0 && contentDips > 0 ? hostSize / contentDips : 1.0;
            return Math.Max(minimum, Math.Min(maximum, displayDips * hostUnitsPerDip));
        }
    }
}
