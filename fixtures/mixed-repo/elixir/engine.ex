defmodule Pricing.Engine do
  @moduledoc "Simple pricing engine for fixtures."

  use GenServer
  alias Pricing.Rules

  def start_link(opts \\ []) do
    GenServer.start_link(__MODULE__, opts, name: __MODULE__)
  end

  def calculate(base, tags) when is_number(base) do
    Rules.apply(base, tags)
  end

  defp normalize(tags), do: Enum.map(tags, &String.downcase/1)
end
