defmodule Aether.MixProject do
  use Mix.Project

  def project do
    [
      app: :aether,
      version: "0.1.0",
      elixir: "~> 1.15",
      description: "Elixir client for the Æther Agent ttrpc service",
      deps: deps()
    ]
  end

  def application do
    [extra_applications: [:logger]]
  end

  defp deps do
    [
      {:protobuf, "~> 0.12"},
    ]
  end
end
