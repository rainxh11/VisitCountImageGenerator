using System.Globalization;
using LiteDB;
using LiteDB.Async;
using Microsoft.AspNetCore.Mvc;
using Serilog;
using Serilog.Events;
using Serilog.Sinks.SystemConsole.Themes;
using SixLabors.Fonts;
using SixLabors.ImageSharp;
using SixLabors.ImageSharp.Drawing.Processing;
using SixLabors.ImageSharp.Processing;
using VisitCountImageGenerator;
using Image = SixLabors.ImageSharp.Image;


var builder = WebApplication.CreateBuilder(args);

#region Logging

var logger = new LoggerConfiguration()
            .MinimumLevel.Override("Microsoft", LogEventLevel.Verbose)
            .Enrich.FromLogContext()
            .WriteTo.Console(theme: AnsiConsoleTheme.Code, restrictedToMinimumLevel: LogEventLevel.Information);

Log.Logger = logger.CreateLogger();
builder.Host.UseSerilog();
builder.Services.AddSerilog();

#endregion

builder.Services
       .AddLogging()
       .ConfigureHttpJsonOptions(options => options.SerializerOptions.WriteIndented = true)
       .AddSingleton<ILiteDatabaseAsync>(new LiteDatabaseAsync(builder.Configuration["ConnectionString"]))
       .AddSingleton<Font>(provider =>
                           {
                             FontCollection collection = new();
                             var family = collection.Add(Path.Combine(AppContext.BaseDirectory, "Caskaydia.otf"));
                             var font = family.CreateFont(48, FontStyle.Regular);
                             return font;
                           })
       .AddSingleton<CounterImage>();

var app = builder.Build();

app.Use(async (ctx,
               next) =>
        {
          try
          {
            ctx.Response.Headers.Append("Cache-Control", "max-age=0, no-cache, no-store, must-revalidate");
            await next(ctx);
          }
          catch (Exception ex)
          {
            ctx.Response.StatusCode = 500;
            await ctx.Response.CompleteAsync();
          }
        });

app.MapGet("/visit-count/query", async (
                                   [FromServices] ILiteDatabaseAsync db) =>
                                 {
                                   var visits = await db.GetCollection<Visit>()
                                                        .FindAllAsync();

                                   return new
                                          {
                                            Stats = visits.GroupBy(x => x.Username,
                                                                   x => ((string[])x.Request["Headers"])
                                                                    .FirstOrDefault(s => s?.Contains("X-Forwarded-For") is true, null))
                                                          .ToDictionary(x => x.Key,
                                                                        x => new
                                                                             {
                                                                               Count = visits.Count(visit => visit is not null),
                                                                               Visitors = x.Where(s => s is not null)
                                                                             }),
                                            Grouped = visits.GroupBy(x => x.Username).ToDictionary(x => x.Key, x => x)
                                          };
                                 });

app.MapGet("/visit-count/query/{username}", async (
                                              string username,
                                              [FromServices] ILiteDatabaseAsync db) =>
                                            {
                                              var visits = await db.GetCollection<Visit>()
                                                                   .Query()
                                                                   .Where(x => x.Username == username)
                                                                   .ToListAsync();

                                              return visits.OrderByDescending(x => x.CreatedOn);
                                            });

app.MapGet("/visit-count/generate/{username}.png", async (
                                                     string username,
                                                     [FromServices] ILiteDatabaseAsync db,
                                                     [FromServices] CounterImage counter,
                                                     [FromServices] ILogger<Visit> logger,
                                                     HttpRequest request) =>
                                                   {
                                                     var requestDict = new Dictionary<string, object>
                                                                       {
                                                                         { "Cookies", request.Cookies.ToList() },
                                                                         { "Headers", request.Headers.Select(x => x.ToString()).ToList() },
                                                                         { "Host", request.Host },
                                                                         { "Query", request.Query.Keys.ToList() },
                                                                         { "Path", request.Path }
                                                                       };


                                                     var visit = new Visit { Request = requestDict, Username = username };
                                                     await db.GetCollection<Visit>()
                                                             .InsertAsync(visit);

                                                     var count = await db.GetCollection<Visit>()
                                                                         .Query()
                                                                         .Where(x => x.Username == username)
                                                                         .LongCountAsync();

                                                     var image = await counter.GenerateImage($"{count:N0}");

                                                     logger?.LogInformation("For: {@userName} | View Count: {@viewCount:N0}", username, count);

                                                     return Results.File(image, "image/png");
                                                   });

app.Run();

internal class Visit
{
  public string Username { get; set; }
  public Dictionary<string, object> Request { get; set; }
  [BsonId(true)] public int Id { get; set; }
  public DateTime CreatedOn { get; set; } = DateTime.Now;
}

internal class CounterImage
{
  private readonly Font _font;

  public CounterImage(Font font)
  {
    _font = font;
  }

  public async Task<byte[]> GenerateImage(string text)
  {
    var imageBytes = await File.ReadAllBytesAsync(Path.Combine(AppContext.BaseDirectory, "VisitCount.png"));
    using var image = Image.Load(imageBytes);
    const int y = 10;
    const int x = 1080;

    var options = new RichTextOptions(_font)
                  {
                    Origin = new PointF(x, y),
                    HorizontalAlignment = HorizontalAlignment.Right
                  };

    var grayOptions = new RichTextOptions(_font)
                      {
                        Origin = new PointF(825, y),
                        HorizontalAlignment = HorizontalAlignment.Left
                      };

    var brush = Brushes.Solid(Color.Cyan);
    var grayBrush = Brushes.Solid(Color.Gray);
    var pen = Pens.DashDot(Color.Cyan, 5);

    var grayText = "0".PadLeft(9 - text.Length, '0');
    image.Mutate(imageProcessingContext => imageProcessingContext.DrawText(options, text, brush)
                                                                 .DrawText(grayOptions, grayText, grayBrush)
                );

    var result = image.Clone(context => context.ConvertToAvatar(image.Size, 20));

    var stream = new MemoryStream();
    await result.SaveAsPngAsync(stream);
    return stream.ToArray();
  }
}