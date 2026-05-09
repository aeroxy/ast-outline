require_relative 'helpers'

class LibWrapper
  def call
    Helpers.greet('lib')
  end
end
