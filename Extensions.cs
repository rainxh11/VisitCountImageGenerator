using SixLabors.ImageSharp;
using SixLabors.ImageSharp.Drawing;
using SixLabors.ImageSharp.Drawing.Processing;
using SixLabors.ImageSharp.PixelFormats;
using SixLabors.ImageSharp.Processing;

namespace VisitCountImageGenerator;

public static class Extensions
{
    public static IImageProcessingContext ConvertToAvatar(this IImageProcessingContext processingContext,
                                                          Size size,
                                                          float cornerRadius)
    {
        return processingContext.Resize(new ResizeOptions
                                        {
                                            Size = size,
                                            Mode = ResizeMode.Crop
                                        }).ApplyRoundedCorners(cornerRadius);
    }

    public static IImageProcessingContext ApplyRoundedCorners(this IImageProcessingContext ctx,
                                                              float cornerRadius)
    {
        var size    = ctx.GetCurrentSize();
        var corners = BuildCorners(size.Width, size.Height, cornerRadius);

        ctx.SetGraphicsOptions(new GraphicsOptions()
                               {
                                   Antialias = true,
                                   AlphaCompositionMode =
                                       PixelAlphaCompositionMode
                                          .DestOut // enforces that any part of this shape that has color is punched out of the background
                               });

        // mutating in here as we already have a cloned original
        // use any color (not Transparent), so the corners will be clipped
        foreach (var c in corners) ctx = ctx.Fill(Color.Red, c);

        return ctx;
    }

    public static IPathCollection BuildCorners(int imageWidth,
                                               int imageHeight,
                                               float cornerRadius)
    {
        // first create a square
        var rect = new RectangularPolygon(-0.5f, -0.5f, cornerRadius, cornerRadius);

        // then cut out of the square a circle so we are left with a corner
        var cornerTopLeft = rect.Clip(new EllipsePolygon(cornerRadius - 0.5f, cornerRadius - 0.5f, cornerRadius));

        // corner is now a corner shape positions top left
        //lets make 3 more positioned correctly, we can do that by translating the original around the center of the image

        var rightPos  = imageWidth - cornerTopLeft.Bounds.Width + 1;
        var bottomPos = imageHeight - cornerTopLeft.Bounds.Height + 1;

        // move it across the width of the image - the width of the shape
        var cornerTopRight    = cornerTopLeft.RotateDegree(90).Translate(rightPos, 0);
        var cornerBottomLeft  = cornerTopLeft.RotateDegree(-90).Translate(0, bottomPos);
        var cornerBottomRight = cornerTopLeft.RotateDegree(180).Translate(rightPos, bottomPos);

        return new PathCollection(cornerTopLeft, cornerBottomLeft, cornerTopRight, cornerBottomRight);
    }
}