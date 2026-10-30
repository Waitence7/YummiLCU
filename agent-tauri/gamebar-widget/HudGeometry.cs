using System;
using System.Threading.Tasks;

namespace YummiGameBarWidget
{
    internal static class HudGeometry
    {
        internal static double ToLogicalPixels(double physicalPixels, double scale)
        {
            if (physicalPixels <= 0 || scale <= 0 || double.IsNaN(scale) || double.IsInfinity(scale))
                throw new ArgumentOutOfRangeException("display", "Display size and scale must be valid.");
            return physicalPixels / scale;
        }

        internal struct FitResult
        {
            internal double Width;
            internal double Height;
            internal bool Accepted;
        }

        // Probe only in response to a fit request. The host can reject sizes
        // below our manifest maximum because it reserves room for its chrome.

        // Grow only from the host's current accepted size toward the requested
        // display target. A temporary Game Bar chrome limit must never make us
        // shrink below the size the user/host already accepted.
        internal static async Task<FitResult> FitLargestUpwardAsync(double targetWidth, double targetHeight,
            double currentWidth, double currentHeight, Func<double, double, Task<bool>> resize)
        {
            var lowWidth = Math.Min(targetWidth, Math.Max(1.0, currentWidth));
            var lowHeight = Math.Min(targetHeight, Math.Max(1.0, currentHeight));

            if (currentWidth >= targetWidth && currentHeight >= targetHeight)
                return new FitResult { Width = currentWidth, Height = currentHeight, Accepted = true };

            if (await resize(targetWidth, targetHeight))
                return new FitResult { Width = targetWidth, Height = targetHeight, Accepted = true };

            // A rejected full target does not tell us which axis was rejected.
            // Probe width at the already-accepted current height first, then probe
            // height at the fitted width. This lets 1440x820 succeed even when
            // 1440x900 is temporarily rejected only because of height.
            var fittedWidth = lowWidth;
            if (targetWidth > fittedWidth && await resize(targetWidth, lowHeight))
            {
                fittedWidth = targetWidth;
            }
            else
            {
                var highWidth = targetWidth;
                for (int attempt = 0; attempt < 14 && highWidth - fittedWidth > 1.0; attempt++)
                {
                    var mid = Math.Floor((fittedWidth + highWidth) / 2.0);
                    if (mid <= fittedWidth) break;
                    if (await resize(mid, lowHeight)) fittedWidth = mid;
                    else highWidth = mid;
                }
            }

            var fittedHeight = lowHeight;
            if (targetHeight > fittedHeight && await resize(fittedWidth, targetHeight))
            {
                fittedHeight = targetHeight;
            }
            else
            {
                var highHeight = targetHeight;
                for (int attempt = 0; attempt < 14 && highHeight - fittedHeight > 1.0; attempt++)
                {
                    var mid = Math.Floor((fittedHeight + highHeight) / 2.0);
                    if (mid <= fittedHeight) break;
                    if (await resize(fittedWidth, mid)) fittedHeight = mid;
                    else highHeight = mid;
                }
            }

            var accepted = await resize(fittedWidth, fittedHeight);
            return new FitResult { Width = fittedWidth, Height = fittedHeight, Accepted = accepted };
        }

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

    }
}
